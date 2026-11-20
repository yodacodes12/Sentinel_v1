//! System and user services, from read-only systemd listings.

pub mod query;

use std::collections::BTreeMap;

use serde::Deserialize;

use sentinel_hardware::SysRoot;
use sentinel_inventory::{ServiceInfo, ServiceScope};

use crate::catalogue::service_category;
use query::{Query, QueryError};

#[derive(Debug, Deserialize)]
struct UnitRow {
    unit: String,
    load: Option<String>,
    active: Option<String>,
    sub: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UnitFileRow {
    unit_file: String,
    state: Option<String>,
}

const SYSTEM_UNIT_DIRS: &[&str] = &[
    "etc/systemd/system",
    "run/systemd/system",
    "usr/local/lib/systemd/system",
    "usr/lib/systemd/system",
    "lib/systemd/system",
];
const USER_UNIT_DIRS: &[&str] = &[
    "etc/systemd/user",
    "usr/lib/systemd/user",
    "lib/systemd/user",
];

/// Merge `list-units` and `list-unit-files` output into service records.
pub fn parse(
    units_json: &str,
    unit_files_json: Option<&str>,
    scope: ServiceScope,
) -> Result<Vec<ServiceInfo>, String> {
    let units: Vec<UnitRow> = serde_json::from_str(units_json).map_err(|e| e.to_string())?;
    let files: Vec<UnitFileRow> = match unit_files_json {
        Some(text) => serde_json::from_str(text).map_err(|e| e.to_string())?,
        None => Vec::new(),
    };
    let mut enabled: BTreeMap<String, Option<String>> =
        files.into_iter().map(|f| (f.unit_file, f.state)).collect();
    let mut services: Vec<ServiceInfo> = units
        .into_iter()
        .filter(|u| u.unit.ends_with(".service"))
        .map(|u| ServiceInfo {
            enabled_state: enabled.remove(&u.unit).flatten(),
            relevant_category: service_category(&u.unit),
            name: u.unit,
            description: u.description.filter(|d| !d.is_empty()),
            scope,
            load_state: u.load,
            active_state: u.active,
            sub_state: u.sub,
            unit_file_path: None,
        })
        .collect();
    // Installed but not loaded (disabled, never started): files only.
    for (name, state) in enabled {
        if name.ends_with(".service") && !name.contains("@.") {
            services.push(ServiceInfo {
                relevant_category: service_category(&name),
                name,
                description: None,
                scope,
                load_state: None,
                active_state: Some("inactive".into()),
                sub_state: None,
                enabled_state: state,
                unit_file_path: None,
            });
        }
    }
    services.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(services)
}

fn locate_unit_files(root: &SysRoot, services: &mut [ServiceInfo], dirs: &[String]) {
    for service in services {
        service.unit_file_path = dirs
            .iter()
            .map(|d| format!("{d}/{}", service.name))
            .find(|rel| root.exists(rel))
            .map(|rel| root.display(&rel));
    }
}

pub struct ServiceScan {
    pub services: Vec<ServiceInfo>,
    pub notes: Vec<String>,
    pub system_ok: bool,
}

/// Run the read-only queries. Only possible on the real machine: with a
/// fixture root there is no systemd to ask, and that is reported.
pub fn discover(
    root: &SysRoot,
    path_dirs: &[std::path::PathBuf],
    config_home: Option<&str>,
) -> ServiceScan {
    let mut scan = ServiceScan {
        services: Vec::new(),
        notes: Vec::new(),
        system_ok: false,
    };
    if !root.is_host() {
        scan.notes
            .push("services are only queried on the real machine, not a test root".into());
        return scan;
    }
    let Some(program) = query::locate(path_dirs) else {
        scan.notes
            .push("systemd is not installed; services cannot be listed".into());
        return scan;
    };
    for (scope, units, files) in [
        (
            ServiceScope::System,
            Query::SystemUnits,
            Query::SystemUnitFiles,
        ),
        (ServiceScope::User, Query::UserUnits, Query::UserUnitFiles),
    ] {
        let label = match scope {
            ServiceScope::System => "system",
            ServiceScope::User => "user",
        };
        let listed = query::run(&program, units);
        let file_list = query::run(&program, files).ok();
        match listed.map_err(|e| match e {
            QueryError::TimedOut => "timed out".to_string(),
            QueryError::NotInstalled => "systemd is not installed".to_string(),
            QueryError::Failed(r) => r,
        }) {
            Ok(text) => match parse(&text, file_list.as_deref(), scope) {
                Ok(mut services) => {
                    let mut dirs: Vec<String> = Vec::new();
                    if scope == ServiceScope::User {
                        if let Some(rel) =
                            config_home.and_then(|h| SysRoot::rel_of(std::path::Path::new(h)))
                        {
                            dirs.push(format!("{rel}/systemd/user"));
                        }
                        dirs.extend(USER_UNIT_DIRS.iter().map(|d| d.to_string()));
                    } else {
                        dirs.extend(SYSTEM_UNIT_DIRS.iter().map(|d| d.to_string()));
                    }
                    locate_unit_files(root, &mut services, &dirs);
                    if scope == ServiceScope::System {
                        scan.system_ok = true;
                    }
                    scan.services.extend(services);
                }
                Err(reason) => scan
                    .notes
                    .push(format!("{label} service list was unreadable: {reason}")),
            },
            Err(reason) => scan
                .notes
                .push(format!("{label} services could not be listed: {reason}")),
        }
    }
    scan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_files_are_merged() {
        let units = r#"[{"unit":"NetworkManager.service","load":"loaded","active":"active","sub":"running","description":"Network Manager"},
                        {"unit":"cups.socket","load":"loaded","active":"active","sub":"listening","description":"CUPS"},
                        {"unit":"foo.service","load":"not-found","active":"inactive","sub":"dead","description":""}]"#;
        let files = r#"[{"unit_file":"NetworkManager.service","state":"enabled","preset":"enabled"},
                        {"unit_file":"smartmontools.service","state":"disabled","preset":"enabled"},
                        {"unit_file":"getty@.service","state":"static","preset":null}]"#;
        let services = parse(units, Some(files), ServiceScope::System).unwrap();
        let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "NetworkManager.service",
                "foo.service",
                "smartmontools.service"
            ]
        );
        assert_eq!(services[0].enabled_state.as_deref(), Some("enabled"));
        assert_eq!(services[0].active_state.as_deref(), Some("active"));
        assert!(services[0].relevant_category.is_some());
        assert_eq!(services[1].description, None);
        assert_eq!(services[2].active_state.as_deref(), Some("inactive"));
        assert_eq!(services[2].enabled_state.as_deref(), Some("disabled"));
    }

    #[test]
    fn malformed_output_is_an_error() {
        assert!(parse("not json", None, ServiceScope::User).is_err());
    }

    #[test]
    fn fixture_roots_do_not_query_systemd() {
        let dir = tempfile::tempdir().unwrap();
        let scan = discover(&SysRoot::at(dir.path()), &[], None);
        assert!(scan.services.is_empty());
        assert!(!scan.system_ok);
        assert_eq!(scan.notes.len(), 1);
    }
}
