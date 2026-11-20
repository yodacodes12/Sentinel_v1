//! Installed-package databases, read without the package managers.
//!
//! Each adapter only reads the on-disk database: dpkg's `status` file is
//! streamed line by line, pacman's `local/` directory names are listed,
//! the RPM SQLite database is opened read-only and immutable (no lock or
//! journal file is created), Flatpak and Snap install directories are
//! listed. No package manager program is run, and only the packages
//! Sentinel looks for are kept in memory.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

use sentinel_hardware::SysRoot;
use sentinel_inventory::{PackageManager, PackageManagerStatus, PackageRef};

const DPKG_STATUS: &str = "var/lib/dpkg/status";
const PACMAN_LOCAL: &str = "var/lib/pacman/local";
const RPM_SQLITE: &str = "var/lib/rpm/rpmdb.sqlite";
const RPM_SQLITE_USR: &str = "usr/lib/sysimage/rpm/rpmdb.sqlite";
const RPM_BDB: &str = "var/lib/rpm/Packages";
const FLATPAK_SYSTEM: &str = "var/lib/flatpak/app";
const SNAP_DIR: &str = "snap";

#[derive(Debug, Default)]
pub struct PackageScan {
    pub statuses: Vec<PackageManagerStatus>,
    /// Wanted package name -> first installed match.
    pub found: BTreeMap<String, PackageRef>,
}

impl PackageScan {
    fn record(&mut self, manager: PackageManager, name: &str, version: Option<String>) {
        self.found.entry(name.to_string()).or_insert(PackageRef {
            manager,
            name: name.to_string(),
            version,
        });
    }
}

/// Parse dpkg `status` text for wanted packages. Returns
/// `(installed package count, wanted -> version)`.
pub fn parse_dpkg<'a>(
    lines: impl Iterator<Item = &'a str>,
    wanted: &BTreeSet<&str>,
) -> (u32, BTreeMap<String, Option<String>>) {
    let mut count = 0;
    let mut found = BTreeMap::new();
    let (mut name, mut installed, mut version): (Option<String>, bool, Option<String>) =
        (None, false, None);
    let mut flush =
        |name: &mut Option<String>, installed: &mut bool, version: &mut Option<String>| {
            if *installed {
                count += 1;
                if let Some(n) = name.as_deref()
                    && wanted.contains(n)
                {
                    found.insert(n.to_string(), version.clone());
                }
            }
            *name = None;
            *installed = false;
            *version = None;
        };
    for line in lines {
        if line.is_empty() {
            flush(&mut name, &mut installed, &mut version);
        } else if let Some(v) = line.strip_prefix("Package: ") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Status: ") {
            installed = v.trim().ends_with(" installed") && v.contains("install ok");
        } else if let Some(v) = line.strip_prefix("Version: ") {
            version = Some(v.trim().to_string());
        }
    }
    flush(&mut name, &mut installed, &mut version);
    (count, found)
}

/// `name-version-release` directory name of a pacman package.
pub fn parse_pacman_dir(dir: &str) -> Option<(String, String)> {
    let mut parts = dir.rsplitn(3, '-');
    let release = parts.next()?;
    let version = parts.next()?;
    let name = parts.next()?;
    (!name.is_empty()).then(|| (name.to_string(), format!("{version}-{release}")))
}

pub fn snap_version(yaml: &str) -> Option<String> {
    yaml.lines()
        .find_map(|l| l.strip_prefix("version:"))
        .map(|v| v.trim().trim_matches(['\'', '"']).to_string())
        .filter(|v| !v.is_empty())
}

pub fn discover(root: &SysRoot, wanted: &BTreeSet<&str>, data_home: Option<&str>) -> PackageScan {
    let mut scan = PackageScan::default();

    // dpkg
    if root.is_file(DPKG_STATUS) {
        let mut lines = Vec::new();
        let mut count = 0;
        let mut found = BTreeMap::new();
        // Stream in stanza-sized batches so the file is never held whole.
        let result = root.for_each_line(DPKG_STATUS, |line| {
            lines.push(line.to_string());
            if line.is_empty() {
                let (c, f) = parse_dpkg(lines.iter().map(String::as_str), wanted);
                count += c;
                found.extend(f);
                lines.clear();
            }
            ControlFlow::Continue(())
        });
        let (c, f) = parse_dpkg(lines.iter().map(String::as_str), wanted);
        count += c;
        found.extend(f);
        for (name, version) in found {
            scan.record(PackageManager::Dpkg, &name, version);
        }
        scan.statuses.push(PackageManagerStatus {
            manager: PackageManager::Dpkg,
            database_found: true,
            installed_packages: result.is_ok().then_some(count),
            note: result
                .err()
                .map(|e| format!("status file could not be read: {e:?}")),
        });
    } else {
        scan.statuses.push(absent(PackageManager::Dpkg));
    }

    // pacman
    match root.list(PACMAN_LOCAL) {
        Ok(dirs) => {
            let mut count = 0;
            for dir in dirs {
                if let Some((name, version)) = parse_pacman_dir(&dir) {
                    count += 1;
                    if wanted.contains(name.as_str()) {
                        scan.record(PackageManager::Pacman, &name, Some(version));
                    }
                }
            }
            scan.statuses.push(PackageManagerStatus {
                manager: PackageManager::Pacman,
                database_found: true,
                installed_packages: Some(count),
                note: None,
            });
        }
        Err(_) => scan.statuses.push(absent(PackageManager::Pacman)),
    }

    // RPM
    let rpm = [RPM_SQLITE, RPM_SQLITE_USR]
        .into_iter()
        .find(|p| root.is_file(p));
    match rpm {
        Some(rel) => {
            let path = root.root().join(rel);
            match rpm_names(&path) {
                Ok(names) => {
                    for name in names.iter().filter(|n| wanted.contains(n.as_str())) {
                        scan.record(PackageManager::Rpm, name, None);
                    }
                    scan.statuses.push(PackageManagerStatus {
                        manager: PackageManager::Rpm,
                        database_found: true,
                        installed_packages: Some(names.len() as u32),
                        note: Some("versions are not read from the RPM database".into()),
                    });
                }
                Err(reason) => scan.statuses.push(PackageManagerStatus {
                    manager: PackageManager::Rpm,
                    database_found: true,
                    installed_packages: None,
                    note: Some(format!("RPM database could not be read: {reason}")),
                }),
            }
        }
        None if root.is_file(RPM_BDB) => scan.statuses.push(PackageManagerStatus {
            manager: PackageManager::Rpm,
            database_found: true,
            installed_packages: None,
            note: Some(
                "this RPM database uses the older Berkeley DB format, which Sentinel does not read"
                    .into(),
            ),
        }),
        None => scan.statuses.push(absent(PackageManager::Rpm)),
    }

    // Flatpak: system and user installations.
    let mut flatpak_dirs = vec![FLATPAK_SYSTEM.to_string()];
    if let Some(home) = data_home.and_then(|h| SysRoot::rel_of(std::path::Path::new(h))) {
        flatpak_dirs.push(format!("{home}/flatpak/app"));
    }
    let mut flatpak_found = false;
    let mut flatpak_count = 0;
    for dir in flatpak_dirs {
        if let Ok(apps) = root.list(&dir) {
            flatpak_found = true;
            for app in apps {
                flatpak_count += 1;
                if wanted.contains(app.as_str()) {
                    scan.record(PackageManager::Flatpak, &app, None);
                }
            }
        }
    }
    scan.statuses.push(PackageManagerStatus {
        manager: PackageManager::Flatpak,
        database_found: flatpak_found,
        installed_packages: flatpak_found.then_some(flatpak_count),
        note: None,
    });

    // Snap: /snap/<name>/current/meta/snap.yaml
    match root.list(SNAP_DIR) {
        Ok(entries) => {
            let mut count = 0;
            for name in entries.iter().filter(|n| *n != "bin" && *n != "README") {
                if !root.is_dir(&format!("{SNAP_DIR}/{name}/current")) {
                    continue;
                }
                count += 1;
                if wanted.contains(name.as_str()) {
                    let version = root
                        .read(&format!("{SNAP_DIR}/{name}/current/meta/snap.yaml"))
                        .ok()
                        .and_then(|y| snap_version(&y));
                    scan.record(PackageManager::Snap, name, version);
                }
            }
            scan.statuses.push(PackageManagerStatus {
                manager: PackageManager::Snap,
                database_found: true,
                installed_packages: Some(count),
                note: None,
            });
        }
        Err(_) => scan.statuses.push(absent(PackageManager::Snap)),
    }
    scan
}

fn absent(manager: PackageManager) -> PackageManagerStatus {
    PackageManagerStatus {
        manager,
        database_found: false,
        installed_packages: None,
        note: None,
    }
}

/// Package names from an RPM SQLite database, opened read-only and
/// immutable so SQLite creates no lock, journal or WAL file next to it.
fn rpm_names(path: &std::path::Path) -> Result<Vec<String>, String> {
    use rusqlite::{Connection, OpenFlags};
    let uri = format!("file:{}?immutable=1", path.display());
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT key FROM Name")
        .map_err(|e| e.to_string())?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "Package: htop\nStatus: install ok installed\nVersion: 3.4.1-5\n\n\
        Package: btop\nStatus: deinstall ok config-files\nVersion: 1.3.0-1\n\n\
        Package: libfoo\nStatus: install ok installed\nVersion: 1\n\n\
        Package: lshw\nStatus: install ok installed\nVersion: 02.19.git.2021.06.19.996aaad9c7-2build4";

    #[test]
    fn dpkg_only_counts_installed_packages() {
        let wanted: BTreeSet<&str> = ["htop", "btop", "lshw"].into();
        let (count, found) = parse_dpkg(STATUS.lines(), &wanted);
        assert_eq!(count, 3);
        assert_eq!(found["htop"].as_deref(), Some("3.4.1-5"));
        assert!(
            !found.contains_key("btop"),
            "removed packages are not installed"
        );
        assert!(
            found.contains_key("lshw"),
            "last stanza without trailing blank line"
        );
    }

    #[test]
    fn pacman_directory_names() {
        assert_eq!(
            parse_pacman_dir("lm_sensors-3.6.0-3"),
            Some(("lm_sensors".into(), "3.6.0-3".into()))
        );
        assert_eq!(
            parse_pacman_dir("nvme-cli-2.9-1"),
            Some(("nvme-cli".into(), "2.9-1".into()))
        );
        assert_eq!(parse_pacman_dir("broken"), None);
    }

    #[test]
    fn snap_yaml_version() {
        assert_eq!(
            snap_version("name: firefox\nversion: 159.0.1-1\n"),
            Some("159.0.1-1".into())
        );
        assert_eq!(
            snap_version("name: x\nversion: '2.0'\n"),
            Some("2.0".into())
        );
        assert_eq!(snap_version("name: x\n"), None);
    }

    #[test]
    fn fixture_tree_with_several_managers() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(p.join("var/lib/dpkg")).unwrap();
        std::fs::write(p.join("var/lib/dpkg/status"), STATUS).unwrap();
        std::fs::create_dir_all(p.join("var/lib/pacman/local/powertop-2.15-1")).unwrap();
        std::fs::create_dir_all(p.join("snap/firmware-updater/current/meta")).unwrap();
        std::fs::write(
            p.join("snap/firmware-updater/current/meta/snap.yaml"),
            "version: 0+git.1\n",
        )
        .unwrap();
        std::fs::create_dir_all(
            p.join("home/u/.local/share/flatpak/app/io.missioncenter.MissionCenter"),
        )
        .unwrap();
        let wanted: BTreeSet<&str> = [
            "htop",
            "powertop",
            "firmware-updater",
            "io.missioncenter.MissionCenter",
        ]
        .into();
        let scan = discover(&SysRoot::at(p), &wanted, Some("/home/u/.local/share"));
        assert_eq!(scan.found["htop"].manager, PackageManager::Dpkg);
        assert_eq!(scan.found["powertop"].manager, PackageManager::Pacman);
        assert_eq!(
            scan.found["firmware-updater"].version.as_deref(),
            Some("0+git.1")
        );
        assert_eq!(
            scan.found["io.missioncenter.MissionCenter"].manager,
            PackageManager::Flatpak
        );
        let rpm = scan
            .statuses
            .iter()
            .find(|s| s.manager == PackageManager::Rpm)
            .unwrap();
        assert!(!rpm.database_found);
    }
}
