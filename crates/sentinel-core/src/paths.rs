//! Resolution of Sentinel's own directories.
//!
//! Follows the XDG Base Directory specification: an environment variable is
//! honoured only when it holds an absolute path, otherwise the documented
//! default under `$HOME` is used. Nothing here hardcodes a home directory.
//!
//! | Purpose        | Location                                             |
//! |----------------|------------------------------------------------------|
//! | Configuration  | `$XDG_CONFIG_HOME/sentinel` (`~/.config/sentinel`)   |
//! | Data (SQLite)  | `$XDG_DATA_HOME/sentinel` (`~/.local/share/sentinel`)|
//! | Cache          | `$XDG_CACHE_HOME/sentinel` (`~/.cache/sentinel`)     |
//! | State and logs | `$XDG_STATE_HOME/sentinel` (`~/.local/state/sentinel`)|
//! | Socket, lock   | `$XDG_RUNTIME_DIR/sentinel`, else `<state>/run`      |

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::error::{Result, SentinelError};
use crate::{APP_DIR_NAME, platform};

/// Longest path accepted for a Unix socket address on Linux
/// (`sizeof(sockaddr_un.sun_path)` minus the terminating NUL).
pub const MAX_SOCKET_PATH_BYTES: usize = 107;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

impl AppPaths {
    /// Resolve from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::resolve(|key| std::env::var_os(key))
    }

    /// Resolve from an arbitrary variable lookup. Tests use this to avoid
    /// touching the real environment.
    pub fn resolve(get: impl Fn(&str) -> Option<OsString>) -> Result<Self> {
        let absolute = |key: &str| -> Option<PathBuf> {
            get(key)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };

        let home = absolute("HOME");
        let base = |key: &str, fallback: &str| -> Result<PathBuf> {
            if let Some(dir) = absolute(key) {
                return Ok(dir.join(APP_DIR_NAME));
            }
            match &home {
                Some(home) => Ok(home.join(fallback).join(APP_DIR_NAME)),
                None => Err(SentinelError::UnsupportedPlatform(format!(
                    "neither {key} nor HOME is set to an absolute path, so Sentinel cannot \
                     locate its directories"
                ))),
            }
        };

        let state_dir = base("XDG_STATE_HOME", ".local/state")?;
        let runtime_dir = match absolute("XDG_RUNTIME_DIR") {
            Some(dir) => dir.join(APP_DIR_NAME),
            None => state_dir.join("run"),
        };

        Ok(Self {
            config_dir: base("XDG_CONFIG_HOME", ".config")?,
            data_dir: base("XDG_DATA_HOME", ".local/share")?,
            cache_dir: base("XDG_CACHE_HOME", ".cache")?,
            state_dir,
            runtime_dir,
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn database_file(&self) -> PathBuf {
        self.data_dir.join("sentinel.db")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }

    pub fn daemon_log_file(&self) -> PathBuf {
        self.log_dir().join("daemon.log")
    }

    pub fn socket_path(&self) -> PathBuf {
        self.runtime_dir.join("daemon.sock")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.runtime_dir.join("daemon.lock")
    }

    /// The socket path, checked against the kernel's address length limit so
    /// the failure is a clear message rather than an opaque bind error.
    pub fn checked_socket_path(&self) -> Result<PathBuf> {
        let path = self.socket_path();
        let len = path.as_os_str().len();
        if len > MAX_SOCKET_PATH_BYTES {
            return Err(SentinelError::UnsupportedPlatform(format!(
                "the IPC socket path {} is {len} bytes long; Linux allows at most \
                 {MAX_SOCKET_PATH_BYTES}. Set XDG_RUNTIME_DIR to a shorter path",
                path.display()
            )));
        }
        Ok(path)
    }

    /// Create every directory the daemon writes to, each private to the user.
    pub fn ensure_all(&self) -> Result<()> {
        for dir in [
            &self.config_dir,
            &self.data_dir,
            &self.cache_dir,
            &self.state_dir,
            &self.log_dir(),
            &self.runtime_dir,
        ] {
            ensure_private_dir(dir)?;
        }
        Ok(())
    }
}

/// Create `path` (and missing parents) with mode `0700` and verify the result
/// is a real directory owned by the current user that nobody else can enter.
///
/// An existing Sentinel directory with looser permissions is tightened; it is
/// application-owned, so this is not a system modification. A directory owned
/// by someone else is refused rather than used.
pub fn ensure_private_dir(path: &Path) -> Result<()> {
    let fail = |reason: String| SentinelError::Directory {
        path: path.to_path_buf(),
        reason,
    };

    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|e| SentinelError::io("creating directory", path, e))?;

    // symlink_metadata: a symlink planted where our directory should be is
    // rejected instead of followed.
    let meta = fs::symlink_metadata(path).map_err(|e| SentinelError::io("inspecting", path, e))?;
    if !meta.file_type().is_dir() {
        return Err(fail("it exists but is not a directory".into()));
    }
    if meta.uid() != platform::current_uid() {
        return Err(fail("it is owned by a different user".into()));
    }
    if meta.mode() & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|e| SentinelError::io("restricting permissions on", path, e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn xdg_variables_take_precedence() {
        let paths = AppPaths::resolve(env(&[
            ("HOME", "/home/u"),
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_DATA_HOME", "/d"),
            ("XDG_CACHE_HOME", "/k"),
            ("XDG_STATE_HOME", "/s"),
            ("XDG_RUNTIME_DIR", "/run/user/1"),
        ]))
        .unwrap();
        assert_eq!(
            paths.config_file(),
            PathBuf::from("/c/sentinel/config.toml")
        );
        assert_eq!(
            paths.database_file(),
            PathBuf::from("/d/sentinel/sentinel.db")
        );
        assert_eq!(paths.cache_dir, PathBuf::from("/k/sentinel"));
        assert_eq!(
            paths.daemon_log_file(),
            PathBuf::from("/s/sentinel/logs/daemon.log")
        );
        assert_eq!(
            paths.socket_path(),
            PathBuf::from("/run/user/1/sentinel/daemon.sock")
        );
        assert_eq!(
            paths.lock_file(),
            PathBuf::from("/run/user/1/sentinel/daemon.lock")
        );
    }

    #[test]
    fn falls_back_to_home_defaults() {
        let paths = AppPaths::resolve(env(&[("HOME", "/home/u")])).unwrap();
        assert_eq!(paths.config_dir, PathBuf::from("/home/u/.config/sentinel"));
        assert_eq!(
            paths.data_dir,
            PathBuf::from("/home/u/.local/share/sentinel")
        );
        assert_eq!(paths.cache_dir, PathBuf::from("/home/u/.cache/sentinel"));
        assert_eq!(
            paths.state_dir,
            PathBuf::from("/home/u/.local/state/sentinel")
        );
        // No XDG_RUNTIME_DIR: the socket lives under the state directory.
        assert_eq!(
            paths.runtime_dir,
            PathBuf::from("/home/u/.local/state/sentinel/run")
        );
    }

    #[test]
    fn relative_xdg_values_are_ignored() {
        let paths = AppPaths::resolve(env(&[
            ("HOME", "/home/u"),
            ("XDG_CONFIG_HOME", "relative/dir"),
        ]))
        .unwrap();
        assert_eq!(paths.config_dir, PathBuf::from("/home/u/.config/sentinel"));
    }

    #[test]
    fn missing_home_and_xdg_is_a_clear_error() {
        let err = AppPaths::resolve(env(&[])).unwrap_err();
        assert!(matches!(err, SentinelError::UnsupportedPlatform(_)));
    }

    #[test]
    fn overlong_socket_path_is_rejected() {
        let long = format!("/{}", "x".repeat(120));
        let paths =
            AppPaths::resolve(env(&[("HOME", "/home/u"), ("XDG_RUNTIME_DIR", &long)])).unwrap();
        assert!(matches!(
            paths.checked_socket_path(),
            Err(SentinelError::UnsupportedPlatform(_))
        ));
    }

    #[test]
    fn ensure_private_dir_creates_with_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a/b/sentinel");
        ensure_private_dir(&dir).unwrap();
        let mode = fs::metadata(&dir).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn ensure_private_dir_tightens_loose_permissions() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("loose");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&dir).unwrap();
        assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
    }

    #[test]
    fn ensure_private_dir_rejects_files_and_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("file");
        fs::write(&file, b"x").unwrap();
        assert!(ensure_private_dir(&file).is_err());

        let target = tmp.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            ensure_private_dir(&link),
            Err(SentinelError::Directory { .. })
        ));
    }

    #[test]
    fn ensure_all_creates_every_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_str().unwrap().to_string();
        let paths = AppPaths::resolve(env(&[("HOME", &root)])).unwrap();
        paths.ensure_all().unwrap();
        for dir in [
            &paths.config_dir,
            &paths.data_dir,
            &paths.cache_dir,
            &paths.log_dir(),
            &paths.runtime_dir,
        ] {
            assert!(dir.is_dir(), "{} missing", dir.display());
        }
    }
}
