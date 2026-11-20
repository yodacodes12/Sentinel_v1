//! XDG desktop entries (`.desktop` files).
//!
//! Only descriptive keys are kept. `Exec` is deliberately not parsed or
//! stored: launching hands the entry's *file path* to the desktop
//! environment's launcher, so Sentinel never interprets a command line
//! taken from a file.

use std::collections::BTreeMap;

use sentinel_hardware::SysRoot;
use sentinel_inventory::{InstallScope, UtilityCategory};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    pub id: String,
    pub path: String,
    pub scope: InstallScope,
    pub name: String,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub categories: Vec<String>,
    pub icon: Option<String>,
    pub terminal: bool,
    pub no_display: bool,
    pub hidden: bool,
    pub try_exec: Option<String>,
}

/// Parse the `[Desktop Entry]` group. `None` for files that are not
/// applications or have no name.
pub fn parse(id: &str, path: &str, scope: InstallScope, text: &str) -> Option<DesktopEntry> {
    let mut in_group = false;
    let mut keys: BTreeMap<&str, &str> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_group || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            // Unlocalised keys only; `Name[de]` and friends are skipped.
            let key = key.trim();
            if !key.contains('[') {
                keys.entry(key).or_insert(value.trim());
            }
        }
    }
    if keys.get("Type").copied().unwrap_or("Application") != "Application" {
        return None;
    }
    let name = keys.get("Name").filter(|n| !n.is_empty())?.to_string();
    let boolean = |key: &str| {
        keys.get(key)
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    };
    let text = |key: &str| keys.get(key).filter(|v| !v.is_empty()).map(|v| unescape(v));
    Some(DesktopEntry {
        id: id.to_string(),
        path: path.to_string(),
        scope,
        name: unescape(&name),
        generic_name: text("GenericName"),
        comment: text("Comment"),
        categories: keys
            .get("Categories")
            .map(|v| {
                v.split(';')
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        icon: text("Icon"),
        terminal: boolean("Terminal"),
        no_display: boolean("NoDisplay"),
        hidden: boolean("Hidden"),
        try_exec: text("TryExec"),
    })
}

fn unescape(value: &str) -> String {
    value
        .replace("\\s", " ")
        .replace("\\n", " ")
        .replace("\\t", " ")
        .replace("\\r", "")
        .replace("\\\\", "\\")
}

/// Category from the entry's freedesktop categories, for entries that are
/// system tools. `None` means "not a system utility" (browsers, games...).
pub fn categorise(entry: &DesktopEntry) -> Option<UtilityCategory> {
    let has = |c: &str| entry.categories.iter().any(|x| x == c);
    if has("TerminalEmulator") || has("FileManager") || has("Development") || has("Game") {
        return None;
    }
    Some(if has("PackageManager") {
        UtilityCategory::PackageManagement
    } else if has("Monitor") && has("Network") {
        UtilityCategory::NetworkDiagnostics
    } else if has("Monitor") {
        UtilityCategory::SystemMonitoring
    } else if has("HardwareSettings") {
        UtilityCategory::HardwareInformation
    } else if has("Network") && (has("Settings") || has("System")) {
        UtilityCategory::NetworkManagement
    } else if has("Archiving") && has("System") {
        UtilityCategory::BackupAndRecovery
    } else if has("Filesystem") || (has("DiskBurning") && has("System")) {
        UtilityCategory::DiskManagement
    } else if has("System") || has("Settings") {
        UtilityCategory::SystemConfiguration
    } else {
        return None;
    })
}

/// Application directories in precedence order: the user's data directory
/// first, then the system data directories.
pub fn application_dirs(
    data_home: Option<&str>,
    data_dirs: &[String],
) -> Vec<(String, InstallScope)> {
    let mut dirs = Vec::new();
    if let Some(home) = data_home {
        dirs.push((
            format!("{}/applications", home.trim_end_matches('/')),
            InstallScope::User,
        ));
    }
    for dir in data_dirs {
        let dir = format!("{}/applications", dir.trim_end_matches('/'));
        let scope = if dir.contains("flatpak") {
            InstallScope::Flatpak
        } else if dir.contains("snapd") {
            InstallScope::Snap
        } else if data_home.is_some_and(|h| dir.starts_with(h.trim_end_matches('/'))) {
            InstallScope::User
        } else {
            InstallScope::System
        };
        if !dirs.iter().any(|(d, _)| *d == dir) {
            dirs.push((dir, scope));
        }
    }
    dirs
}

/// All visible application entries. Earlier directories win; an entry
/// marked `Hidden` there hides the same ID everywhere (per the XDG spec).
pub fn discover(root: &SysRoot, dirs: &[(String, InstallScope)]) -> (Vec<DesktopEntry>, usize) {
    let mut seen: BTreeMap<String, Option<DesktopEntry>> = BTreeMap::new();
    let mut files = 0;
    for (dir, scope) in dirs {
        let Some(rel) = SysRoot::rel_of(std::path::Path::new(dir)) else {
            continue;
        };
        walk(root, &rel, "", 0, &mut |id, rel_file| {
            files += 1;
            if seen.contains_key(&id) {
                return;
            }
            let entry = root
                .read(&rel_file)
                .ok()
                .and_then(|text| parse(&id, &root.display(&rel_file), *scope, &text))
                .filter(|e| !e.hidden);
            seen.insert(id, entry);
        });
    }
    (seen.into_values().flatten().collect(), files)
}

/// Visit `.desktop` files below `rel`, at most 3 levels deep. Desktop IDs of
/// files in subdirectories join the path with `-` (XDG rule).
fn walk(root: &SysRoot, rel: &str, prefix: &str, depth: u8, visit: &mut dyn FnMut(String, String)) {
    for name in root.list(rel).unwrap_or_default() {
        let path = format!("{rel}/{name}");
        if name.ends_with(".desktop") && root.is_file(&path) {
            visit(format!("{prefix}{name}"), path);
        } else if depth < 3 && root.is_dir(&path) {
            walk(root, &path, &format!("{prefix}{name}-"), depth + 1, visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISKS: &str = "[Desktop Entry]\nName=Disks\nName[de]=Laufwerke\n\
        Comment=Manage Drives and Media\nExec=gnome-disks --rm -rf /\nIcon=org.gnome.DiskUtility\n\
        Terminal=false\nType=Application\nCategories=GTK;GNOME;Utility;X-GNOME-Utilities;System;\n\
        [Desktop Action new]\nName=Should be ignored\n";

    #[test]
    fn parses_descriptive_keys_and_never_exec() {
        let entry = parse(
            "org.gnome.DiskUtility.desktop",
            "/usr/share/applications/x.desktop",
            InstallScope::System,
            DISKS,
        )
        .unwrap();
        assert_eq!(entry.name, "Disks");
        assert_eq!(entry.comment.as_deref(), Some("Manage Drives and Media"));
        assert_eq!(entry.icon.as_deref(), Some("org.gnome.DiskUtility"));
        assert!(!entry.terminal);
        assert!(entry.categories.contains(&"System".to_string()));
        assert!(
            !format!("{entry:?}").contains("--rm"),
            "Exec must not be kept"
        );
    }

    #[test]
    fn non_applications_and_nameless_entries_are_skipped() {
        assert!(
            parse(
                "a.desktop",
                "/a",
                InstallScope::System,
                "[Desktop Entry]\nType=Link\nName=x\n"
            )
            .is_none()
        );
        assert!(
            parse(
                "a.desktop",
                "/a",
                InstallScope::System,
                "[Desktop Entry]\nType=Application\n"
            )
            .is_none()
        );
        assert!(parse("a.desktop", "/a", InstallScope::System, "garbage").is_none());
    }

    #[test]
    fn categorisation() {
        let entry = |cats: &str| {
            parse(
                "x.desktop",
                "/x",
                InstallScope::System,
                &format!("[Desktop Entry]\nName=X\nCategories={cats}\n"),
            )
            .unwrap()
        };
        assert_eq!(
            categorise(&entry("System;Monitor;")),
            Some(UtilityCategory::SystemMonitoring)
        );
        assert_eq!(
            categorise(&entry("GNOME;GTK;System;PackageManager;")),
            Some(UtilityCategory::PackageManagement)
        );
        assert_eq!(
            categorise(&entry("Network;Monitor;")),
            Some(UtilityCategory::NetworkDiagnostics)
        );
        assert_eq!(
            categorise(&entry("Settings;HardwareSettings;")),
            Some(UtilityCategory::HardwareInformation)
        );
        assert_eq!(categorise(&entry("System;TerminalEmulator;")), None);
        assert_eq!(categorise(&entry("Network;WebBrowser;")), None);
        assert_eq!(
            categorise(&entry("System;Settings;")),
            Some(UtilityCategory::SystemConfiguration)
        );
    }

    #[test]
    fn directories_and_scopes() {
        let dirs = application_dirs(
            Some("/home/u/.local/share"),
            &[
                "/usr/share".into(),
                "/var/lib/snapd/desktop".into(),
                "/var/lib/flatpak/exports/share".into(),
                "/usr/share/".into(),
            ],
        );
        assert_eq!(
            dirs[0],
            (
                "/home/u/.local/share/applications".into(),
                InstallScope::User
            )
        );
        assert_eq!(dirs[1].1, InstallScope::System);
        assert_eq!(dirs[2].1, InstallScope::Snap);
        assert_eq!(dirs[3].1, InstallScope::Flatpak);
        assert_eq!(dirs.len(), 4, "duplicates removed");
    }

    #[test]
    fn earlier_directories_win_and_hidden_hides() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("home/u/.local/share/applications");
        let system = dir.path().join("usr/share/applications/kde");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(
            user.join("htop.desktop"),
            "[Desktop Entry]\nName=htop\nHidden=true\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("usr/share/applications/htop.desktop"),
            "[Desktop Entry]\nName=htop\n",
        )
        .unwrap();
        std::fs::write(
            system.join("ksysguard.desktop"),
            "[Desktop Entry]\nName=KSysGuard\n",
        )
        .unwrap();
        let root = SysRoot::at(dir.path());
        let dirs = application_dirs(Some("/home/u/.local/share"), &["/usr/share".into()]);
        let (entries, files) = discover(&root, &dirs);
        assert_eq!(files, 3);
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["kde-ksysguard.desktop"]);
    }
}
