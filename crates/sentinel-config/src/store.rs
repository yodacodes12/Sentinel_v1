use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sentinel_core::{Result, SentinelError};

use crate::model::{CONFIG_VERSION, ConfigPatch, SentinelConfig};

/// How the configuration in a [`ConfigStore`] came to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadStatus {
    /// Read from disk unchanged.
    Loaded,
    /// No file existed; defaults are in use.
    Defaulted,
    /// An older file was upgraded from this version.
    Migrated { from: u32 },
    /// The file could not be understood. It was preserved at `backup` (when
    /// the store is allowed to write) and defaults are in use.
    RecoveredFromCorrupt {
        reason: String,
        backup: Option<PathBuf>,
    },
    /// The file was written by a newer Sentinel. It is left untouched,
    /// defaults are used in memory and updates are refused.
    NewerVersion { found: u32 },
}

enum ParseError {
    Corrupt(String),
    Newer(u32),
}

/// Parse configuration text, migrating older versions forward.
///
/// Returns the configuration and, if a migration happened, the version it was
/// migrated from.
pub fn parse(text: &str) -> Result<(SentinelConfig, Option<u32>)> {
    parse_inner(text).map_err(|e| match e {
        ParseError::Corrupt(reason) => SentinelError::InvalidConfig(reason),
        ParseError::Newer(found) => SentinelError::InvalidConfig(format!(
            "configuration version {found} is newer than this Sentinel supports \
             ({CONFIG_VERSION})"
        )),
    })
}

fn parse_inner(text: &str) -> std::result::Result<(SentinelConfig, Option<u32>), ParseError> {
    let mut table: toml::Table =
        toml::from_str(text).map_err(|e| ParseError::Corrupt(format!("not valid TOML: {e}")))?;

    // A file without a version predates versioning: treat it as version 0.
    let found = match table.get("version") {
        None => 0,
        Some(toml::Value::Integer(v)) if (0..=i64::from(u32::MAX)).contains(v) => *v as u32,
        Some(other) => {
            return Err(ParseError::Corrupt(format!(
                "`version` must be a non-negative integer, found {other}"
            )));
        }
    };
    if found > CONFIG_VERSION {
        return Err(ParseError::Newer(found));
    }

    let migrated_from = (found < CONFIG_VERSION).then_some(found);
    migrate(&mut table, found);

    let config: SentinelConfig = toml::Value::Table(table)
        .try_into()
        .map_err(|e| ParseError::Corrupt(format!("invalid setting: {e}")))?;
    config
        .validate()
        .map_err(|e| ParseError::Corrupt(e.to_string()))?;
    Ok((config, migrated_from))
}

/// Step-by-step upgrades. Each arm upgrades exactly one version so future
/// versions only add an arm.
fn migrate(table: &mut toml::Table, from: u32) {
    let mut version = from;
    while version < CONFIG_VERSION {
        match version {
            // v0 -> v1: introduce the `version` field; settings are unchanged
            // and anything missing takes its default during deserialisation.
            0 => {}
            // v1 -> v2 (Phase 3): sampling, alert and retention settings are
            // added with defaults; existing settings are unchanged. Monitoring
            // stays as the user left it.
            1 => {}
            _ => unreachable!("no migration defined from version {version}"),
        }
        version += 1;
    }
    table.insert(
        "version".into(),
        toml::Value::Integer(CONFIG_VERSION.into()),
    );
}

/// The configuration plus where it lives and whether it may be written.
#[derive(Debug)]
pub struct ConfigStore {
    path: PathBuf,
    config: SentinelConfig,
    status: LoadStatus,
}

impl ConfigStore {
    /// Load without writing anything. Used by read-only consumers such as
    /// `sentinel status` while the daemon is stopped.
    pub fn read(path: &Path) -> Result<Self> {
        Self::load(path, false)
    }

    /// Load and bring the file on disk up to date: write defaults when it is
    /// missing, persist migrations, and move a corrupt file aside.
    pub fn open(path: &Path) -> Result<Self> {
        Self::load(path, true)
    }

    fn load(path: &Path, repair: bool) -> Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            // Not UTF-8: the file is corrupt, not unreadable.
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Some(String::from("\u{0}")),
            Err(e) => return Err(SentinelError::io("reading configuration", path, e)),
        };

        let (config, status) = match text.as_deref().map(parse_inner) {
            None => (SentinelConfig::default(), LoadStatus::Defaulted),
            Some(Ok((config, None))) => (config, LoadStatus::Loaded),
            Some(Ok((config, Some(from)))) => (config, LoadStatus::Migrated { from }),
            Some(Err(ParseError::Newer(found))) => (
                SentinelConfig::default(),
                LoadStatus::NewerVersion { found },
            ),
            Some(Err(ParseError::Corrupt(reason))) => {
                let backup = if repair {
                    Some(preserve_corrupt(path)?)
                } else {
                    None
                };
                (
                    SentinelConfig::default(),
                    LoadStatus::RecoveredFromCorrupt { reason, backup },
                )
            }
        };

        let store = Self {
            path: path.to_path_buf(),
            config,
            status,
        };
        if repair
            && matches!(
                store.status,
                LoadStatus::Defaulted
                    | LoadStatus::Migrated { .. }
                    | LoadStatus::RecoveredFromCorrupt { .. }
            )
        {
            store.save()?;
        }
        Ok(store)
    }

    pub fn config(&self) -> &SentinelConfig {
        &self.config
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn status(&self) -> &LoadStatus {
        &self.status
    }

    /// Validate, persist and adopt a partial update. The in-memory value only
    /// changes once the file has been written.
    pub fn update(&mut self, patch: &ConfigPatch) -> Result<&SentinelConfig> {
        if let LoadStatus::NewerVersion { found } = self.status {
            return Err(SentinelError::InvalidConfig(format!(
                "{} was written by a newer Sentinel (version {found}); refusing to overwrite it",
                self.path.display()
            )));
        }
        let next = self.config.with_patch(patch)?;
        write_atomically(&self.path, &next)?;
        self.config = next;
        Ok(&self.config)
    }

    fn save(&self) -> Result<()> {
        write_atomically(&self.path, &self.config)
    }
}

/// Move an unreadable configuration aside so the user can recover it.
fn preserve_corrupt(path: &Path) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".corrupt-{stamp}"));
    let backup = path.with_file_name(name);
    fs::rename(path, &backup)
        .map_err(|e| SentinelError::io("preserving corrupt configuration", path, e))?;
    Ok(backup)
}

/// Write to a private temporary file in the same directory, then rename over
/// the target, so a crash never leaves a half-written configuration.
fn write_atomically(path: &Path, config: &SentinelConfig) -> Result<()> {
    let body = toml::to_string_pretty(config)
        .map_err(|e| SentinelError::InvalidConfig(format!("could not serialise: {e}")))?;
    let text = format!(
        "# Sentinel configuration. Managed by Sentinel; safe to edit while it is stopped.\n{body}"
    );

    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".tmp-{}", std::process::id()));
    let tmp = path.with_file_name(tmp_name);

    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();

    result.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        SentinelError::io("writing configuration", path, e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PowerMode, Theme};
    use std::os::unix::fs::PermissionsExt;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn parses_a_complete_file() {
        let (config, migrated) = parse(
            "version = 2\nmonitoring_enabled = true\npower_mode = \"eco\"\n\
             notifications_enabled = false\ntheme = \"black\"\n",
        )
        .unwrap();
        assert_eq!(migrated, None);
        assert!(config.monitoring_enabled);
        assert_eq!(config.power_mode, PowerMode::Eco);
        assert!(!config.notifications_enabled);
        assert_eq!(config.theme, Theme::Black);
    }

    #[test]
    fn missing_settings_take_defaults() {
        let (config, _) = parse("version = 1\n").unwrap();
        assert_eq!(config, SentinelConfig::default());
    }

    #[test]
    fn rejects_invalid_values() {
        for text in [
            "version = 1\npower_mode = \"turbo\"\n",
            "version = 1\ntheme = 7\n",
            "version = \"one\"\n",
            "version = -1\n",
            "this is not toml",
        ] {
            assert!(
                matches!(parse(text), Err(SentinelError::InvalidConfig(_))),
                "{text:?} should be rejected"
            );
        }
    }

    #[test]
    fn unversioned_file_is_migrated() {
        let (config, migrated) = parse("power_mode = \"performance\"\n").unwrap();
        assert_eq!(migrated, Some(0));
        assert_eq!(config.version, CONFIG_VERSION);
        assert_eq!(config.power_mode, PowerMode::Performance);
    }

    #[test]
    fn open_creates_private_defaults_when_missing() {
        let tmp = dir();
        let path = tmp.path().join("config.toml");
        let store = ConfigStore::open(&path).unwrap();
        assert_eq!(store.status(), &LoadStatus::Defaulted);
        assert_eq!(store.config(), &SentinelConfig::default());
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // A second open reads the same values back.
        assert_eq!(
            ConfigStore::open(&path).unwrap().status(),
            &LoadStatus::Loaded
        );
    }

    #[test]
    fn read_never_writes() {
        let tmp = dir();
        let path = tmp.path().join("config.toml");
        let store = ConfigStore::read(&path).unwrap();
        assert_eq!(store.status(), &LoadStatus::Defaulted);
        assert!(!path.exists());

        fs::write(&path, "garbage = [").unwrap();
        let store = ConfigStore::read(&path).unwrap();
        assert!(matches!(
            store.status(),
            LoadStatus::RecoveredFromCorrupt { backup: None, .. }
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "garbage = [");
    }

    #[test]
    fn corrupt_file_is_preserved_and_replaced() {
        let tmp = dir();
        let path = tmp.path().join("config.toml");
        fs::write(&path, "power_mode = \"warp\"\n").unwrap();

        let store = ConfigStore::open(&path).unwrap();
        let LoadStatus::RecoveredFromCorrupt {
            backup: Some(backup),
            ..
        } = store.status()
        else {
            panic!("expected recovery, got {:?}", store.status());
        };
        assert_eq!(
            fs::read_to_string(backup).unwrap(),
            "power_mode = \"warp\"\n",
            "the user's original file must survive"
        );
        assert_eq!(store.config(), &SentinelConfig::default());
        assert_eq!(parse(&fs::read_to_string(&path).unwrap()).unwrap().1, None);
    }

    #[test]
    fn binary_garbage_is_treated_as_corrupt() {
        let tmp = dir();
        let path = tmp.path().join("config.toml");
        fs::write(&path, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        let store = ConfigStore::open(&path).unwrap();
        assert!(matches!(
            store.status(),
            LoadStatus::RecoveredFromCorrupt { .. }
        ));
    }

    #[test]
    fn outdated_file_is_migrated_on_disk() {
        let tmp = dir();
        let path = tmp.path().join("config.toml");
        fs::write(&path, "theme = \"black\"\n").unwrap();
        let store = ConfigStore::open(&path).unwrap();
        assert_eq!(store.status(), &LoadStatus::Migrated { from: 0 });
        assert_eq!(store.config().theme, Theme::Black);
        let (on_disk, migrated) = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(migrated, None);
        assert_eq!(on_disk.theme, Theme::Black);
    }

    #[test]
    fn newer_file_is_never_overwritten() {
        let tmp = dir();
        let path = tmp.path().join("config.toml");
        let original = "version = 99\nfuture_setting = true\n";
        fs::write(&path, original).unwrap();

        let mut store = ConfigStore::open(&path).unwrap();
        assert_eq!(store.status(), &LoadStatus::NewerVersion { found: 99 });
        assert_eq!(store.config(), &SentinelConfig::default());
        let err = store
            .update(&ConfigPatch {
                theme: Some(Theme::Black),
                ..Default::default()
            })
            .unwrap_err();
        assert!(matches!(err, SentinelError::InvalidConfig(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn updates_persist_across_reopen() {
        let tmp = dir();
        let path = tmp.path().join("config.toml");
        let mut store = ConfigStore::open(&path).unwrap();
        store
            .update(&ConfigPatch {
                power_mode: Some(PowerMode::Performance),
                notifications_enabled: Some(false),
                ..Default::default()
            })
            .unwrap();

        let reopened = ConfigStore::open(&path).unwrap();
        assert_eq!(reopened.config().power_mode, PowerMode::Performance);
        assert!(!reopened.config().notifications_enabled);
        assert!(
            fs::read_dir(tmp.path()).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")),
            "no temporary file may be left behind"
        );
    }

    #[test]
    fn failed_update_leaves_state_unchanged() {
        let tmp = dir();
        let path = tmp.path().join("config.toml");
        let mut store = ConfigStore::open(&path).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        assert!(store.update(&ConfigPatch::default()).is_err());
        assert_eq!(store.config(), &SentinelConfig::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }
}
