//! Discovery of installed system utilities and services.
//!
//! Four read-only sources are combined: XDG desktop entries
//! ([`desktop`]), installed-package databases ([`packages`]), executables
//! in `PATH` directories (checked by name with `stat`, never run), and
//! systemd service listings ([`services`]). See `docs/UTILITY_DISCOVERY.md`.

pub mod catalogue;
pub mod desktop;
pub mod packages;
pub mod services;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

use sentinel_hardware::{CancelToken, Event, Stopped, SysRoot};
use sentinel_inventory::{
    DesktopEntryRef, DiscoverySource, ModuleId, ModuleOutcome, ModuleReport, SystemUtility,
    UTILITY_SCHEMA_VERSION, UtilityCatalog, UtilityInterface,
};

/// Where to look, taken from the user's environment.
#[derive(Debug, Clone, Default)]
pub struct UtilityEnv {
    /// `$XDG_DATA_HOME` (default `~/.local/share`).
    pub data_home: Option<String>,
    /// `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`).
    pub data_dirs: Vec<String>,
    /// `$XDG_CONFIG_HOME` (default `~/.config`).
    pub config_home: Option<String>,
    /// Absolute directories from `$PATH`.
    pub path_dirs: Vec<PathBuf>,
}

/// Standard binary directories checked in addition to `PATH`.
const STANDARD_BIN_DIRS: &[&str] = &[
    "usr/local/bin",
    "usr/bin",
    "bin",
    "usr/local/sbin",
    "usr/sbin",
    "sbin",
    "snap/bin",
];

impl UtilityEnv {
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let home = var("HOME");
        let data_home =
            var("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| format!("{h}/.local/share")));
        let config_home =
            var("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|h| format!("{h}/.config")));
        let data_dirs = var("XDG_DATA_DIRS")
            .map(|v| {
                v.split(':')
                    .filter(|d| d.starts_with('/'))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_else(|| vec!["/usr/local/share".into(), "/usr/share".into()]);
        let path_dirs = std::env::var_os("PATH")
            .map(|p| {
                std::env::split_paths(&p)
                    .filter(|d| d.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            data_home,
            data_dirs,
            config_home,
            path_dirs,
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct UtilityScan {
    pub catalog: UtilityCatalog,
    pub stopped: Option<Stopped>,
}

/// Find an executable by name in `PATH` and the standard directories.
fn find_executable(root: &SysRoot, env: &UtilityEnv, name: &str) -> Option<String> {
    let path_rels = env.path_dirs.iter().filter_map(|d| SysRoot::rel_of(d));
    path_rels
        .chain(STANDARD_BIN_DIRS.iter().map(|d| d.to_string()))
        .map(|dir| format!("{dir}/{name}"))
        .find(|rel| root.is_executable_file(rel))
        .map(|rel| root.display(&rel))
}

pub fn discover(
    root: &SysRoot,
    env: &UtilityEnv,
    now: u64,
    cancel: &CancelToken,
    on_event: &mut dyn FnMut(Event),
) -> UtilityScan {
    let started = Instant::now();
    let mut reports = Vec::new();
    let mut stopped = None;
    let step = |module: ModuleId,
                reports: &mut Vec<ModuleReport>,
                stopped: &mut Option<Stopped>,
                on_event: &mut dyn FnMut(Event),
                work: &mut dyn FnMut() -> (ModuleOutcome, usize, Vec<String>)| {
        if stopped.is_none() {
            *stopped = cancel.check().err();
        }
        let t = Instant::now();
        let (outcome, items, notes) = if stopped.is_some() {
            (
                ModuleOutcome::Cancelled,
                0,
                vec!["the scan stopped before this module ran".to_string()],
            )
        } else {
            on_event(Event::ModuleStarted(module));
            work()
        };
        let report = ModuleReport {
            module,
            outcome,
            items: items as u32,
            duration_ms: t.elapsed().as_millis() as u64,
            finished_at: now_secs(),
            notes,
        };
        on_event(Event::ModuleFinished(report.clone()));
        reports.push(report);
    };

    // 1. Desktop entries.
    let dirs = desktop::application_dirs(env.data_home.as_deref(), &env.data_dirs);
    let mut entries = Vec::new();
    step(
        ModuleId::DesktopEntries,
        &mut reports,
        &mut stopped,
        on_event,
        &mut || {
            let (found, files) = desktop::discover(root, &dirs);
            entries = found;
            let notes = vec![format!(
                "{files} desktop file(s) read in {} application director(ies)",
                dirs.len()
            )];
            (ModuleOutcome::Completed, entries.len(), notes)
        },
    );

    // 2. Package databases (only the packages the catalogue knows).
    let wanted: BTreeSet<&str> = catalogue::KNOWN
        .iter()
        .flat_map(|k| k.packages.iter().copied())
        .collect();
    let mut package_scan = packages::PackageScan::default();
    step(
        ModuleId::Packages,
        &mut reports,
        &mut stopped,
        on_event,
        &mut || {
            package_scan = packages::discover(root, &wanted, env.data_home.as_deref());
            let found = package_scan
                .statuses
                .iter()
                .filter(|s| s.database_found)
                .count();
            if found == 0 {
                (
                    ModuleOutcome::Unavailable,
                    0,
                    vec!["no supported package database was found".into()],
                )
            } else {
                let notes = package_scan
                    .statuses
                    .iter()
                    .filter_map(|s| s.note.clone())
                    .collect();
                (ModuleOutcome::Completed, package_scan.found.len(), notes)
            }
        },
    );

    // 3. Executables, by name only.
    let mut executables: BTreeMap<&str, String> = BTreeMap::new();
    step(
        ModuleId::Executables,
        &mut reports,
        &mut stopped,
        on_event,
        &mut || {
            for known in catalogue::KNOWN {
                for exe in known.executables {
                    if let Some(path) = find_executable(root, env, exe) {
                        executables.insert(exe, path);
                    }
                }
            }
            (ModuleOutcome::Completed, executables.len(), Vec::new())
        },
    );

    // 4. Services.
    let mut service_list = Vec::new();
    step(
        ModuleId::Services,
        &mut reports,
        &mut stopped,
        on_event,
        &mut || {
            let scan = services::discover(root, &env.path_dirs, env.config_home.as_deref());
            service_list = scan.services;
            let outcome = if !scan.system_ok {
                ModuleOutcome::Unavailable
            } else if scan.notes.is_empty() {
                ModuleOutcome::Completed
            } else {
                ModuleOutcome::Partial
            };
            (outcome, service_list.len(), scan.notes)
        },
    );

    let utilities = merge(&entries, &package_scan, &executables);
    UtilityScan {
        catalog: UtilityCatalog {
            schema_version: UTILITY_SCHEMA_VERSION,
            discovered_at: now,
            duration_ms: started.elapsed().as_millis() as u64,
            root_override: (!root.is_host()).then(|| root.root().display().to_string()),
            utilities,
            services: service_list,
            package_managers: package_scan.statuses,
            discovery_status: reports,
        },
        stopped,
    }
}

fn entry_ref(entry: &desktop::DesktopEntry) -> DesktopEntryRef {
    DesktopEntryRef {
        id: entry.id.clone(),
        path: entry.path.clone(),
        scope: entry.scope,
        terminal: entry.terminal,
        no_display: entry.no_display,
    }
}

const CLI_NOTE: &str =
    "Command-line tool: run it yourself in a terminal. Sentinel does not run it.";

fn merge(
    entries: &[desktop::DesktopEntry],
    packages: &packages::PackageScan,
    executables: &BTreeMap<&str, String>,
) -> Vec<SystemUtility> {
    let mut used_entries = BTreeSet::new();
    let mut utilities = Vec::new();
    for known in catalogue::KNOWN {
        let entry = known
            .desktop_ids
            .iter()
            .find_map(|id| entries.iter().find(|e| e.id == *id));
        if let Some(e) = entry {
            used_entries.insert(e.id.clone());
        }
        let package = known
            .packages
            .iter()
            .find_map(|p| packages.found.get(*p))
            .cloned();
        let executable = known
            .executables
            .iter()
            .find_map(|x| executables.get(x))
            .cloned();
        let mut sources = Vec::new();
        if entry.is_some() {
            sources.push(DiscoverySource::DesktopEntry);
        }
        if package.is_some() {
            sources.push(DiscoverySource::PackageDatabase);
        }
        if executable.is_some() {
            sources.push(DiscoverySource::Executable);
        }
        let installed = !sources.is_empty();
        if !installed {
            sources.push(DiscoverySource::Catalogue);
        }
        let launch_available = entry.is_some();
        utilities.push(SystemUtility {
            id: known.id.to_string(),
            name: entry
                .map(|e| e.name.clone())
                .unwrap_or_else(|| known.name.to_string()),
            description: Some(known.description.to_string()),
            category: known.category,
            interface: match entry {
                Some(e) if e.terminal => UtilityInterface::Terminal,
                Some(_) => UtilityInterface::Graphical,
                None => known.interface,
            },
            installed,
            version: package.as_ref().and_then(|p| p.version.clone()),
            executable,
            desktop_entry: entry.map(entry_ref),
            package,
            icon: entry.and_then(|e| e.icon.clone()),
            sources,
            launch_available,
            launch_note: match (installed, launch_available) {
                (false, _) => Some("Not installed on this system.".into()),
                (true, false) => Some(CLI_NOTE.into()),
                _ => None,
            },
        });
    }
    // System-related desktop entries Sentinel does not know by name.
    for entry in entries {
        if used_entries.contains(&entry.id) || entry.no_display {
            continue;
        }
        let Some(category) = desktop::categorise(entry) else {
            continue;
        };
        utilities.push(SystemUtility {
            id: format!("desktop:{}", entry.id.trim_end_matches(".desktop")),
            name: entry.name.clone(),
            description: entry.comment.clone().or_else(|| entry.generic_name.clone()),
            category,
            interface: if entry.terminal {
                UtilityInterface::Terminal
            } else {
                UtilityInterface::Graphical
            },
            installed: true,
            version: None,
            executable: None,
            desktop_entry: Some(entry_ref(entry)),
            package: None,
            icon: entry.icon.clone(),
            sources: vec![DiscoverySource::DesktopEntry],
            launch_available: true,
            launch_note: None,
        });
    }
    utilities.sort_by(|a, b| {
        (a.category, !a.installed, a.name.to_lowercase()).cmp(&(
            b.category,
            !b.installed,
            b.name.to_lowercase(),
        ))
    });
    utilities
}

pub use sentinel_inventory::is_valid_utility_id;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utility_ids() {
        assert!(is_valid_utility_id("htop"));
        assert!(is_valid_utility_id("desktop:org.gnome.DiskUtility"));
        assert!(!is_valid_utility_id("htop; rm -rf ~"));
        assert!(!is_valid_utility_id("../../etc/passwd"));
        assert!(!is_valid_utility_id(""));
    }

    #[test]
    fn fixture_catalogue() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let apps = p.join("usr/share/applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(
            apps.join("htop.desktop"),
            "[Desktop Entry]\nName=Htop\nTerminal=true\nCategories=System;Monitor;\n",
        )
        .unwrap();
        std::fs::write(
            apps.join("org.example.Tool.desktop"),
            "[Desktop Entry]\nName=Tool\nComment=Example tool\nCategories=System;Settings;\n",
        )
        .unwrap();
        std::fs::write(
            apps.join("browser.desktop"),
            "[Desktop Entry]\nName=Browser\nCategories=Network;WebBrowser;\n",
        )
        .unwrap();
        std::fs::create_dir_all(p.join("usr/bin")).unwrap();
        let exe = p.join("usr/bin/lshw");
        std::fs::write(&exe, "").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(p.join("usr/bin/inxi"), "").unwrap(); // not executable

        let env = UtilityEnv {
            data_dirs: vec!["/usr/share".into()],
            ..Default::default()
        };
        let scan = discover(&SysRoot::at(p), &env, 1, &CancelToken::new(), &mut |_| {});
        let get = |id: &str| scan.catalog.utilities.iter().find(|u| u.id == id).unwrap();

        let htop = get("htop");
        assert!(htop.installed && htop.launch_available);
        assert_eq!(htop.interface, UtilityInterface::Terminal);
        let lshw = get("lshw");
        assert!(lshw.installed && !lshw.launch_available);
        assert_eq!(lshw.sources, [DiscoverySource::Executable]);
        assert!(
            !get("inxi").installed,
            "a non-executable file is not an installed tool"
        );
        assert!(!get("btop").installed);
        assert_eq!(get("btop").sources, [DiscoverySource::Catalogue]);
        assert!(get("desktop:org.example.Tool").installed);
        assert!(!scan.catalog.utilities.iter().any(|u| u.name == "Browser"));
        let services = scan
            .catalog
            .discovery_status
            .iter()
            .find(|r| r.module == ModuleId::Services)
            .unwrap();
        assert_eq!(services.outcome, ModuleOutcome::Unavailable);
        assert!(scan.catalog.root_override.is_some());
    }

    #[test]
    fn cancellation_marks_every_module() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let scan = discover(
            &SysRoot::at(dir.path()),
            &UtilityEnv::default(),
            1,
            &cancel,
            &mut |_| {},
        );
        assert_eq!(scan.stopped, Some(Stopped::Cancelled));
        assert!(
            scan.catalog
                .discovery_status
                .iter()
                .all(|r| r.outcome == ModuleOutcome::Cancelled)
        );
    }
}
