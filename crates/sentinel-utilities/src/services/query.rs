//! The read-only systemd queries Sentinel runs.
//!
//! This file is the only place in the discovery crates that starts a
//! process, and the safety suite enforces that. What it can run is closed:
//! [`Query`] has four variants, each mapped to a fixed argument vector made
//! of the listing verbs `list-units` and `list-unit-files` plus output
//! flags. No argument comes from outside this file, no shell is involved,
//! the environment is cleared, and a slow query is abandoned (left to finish
//! and be reaped) rather than killed.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub const PROGRAM: &str = "systemctl";
const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OUTPUT: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Query {
    SystemUnits,
    SystemUnitFiles,
    UserUnits,
    UserUnitFiles,
}

impl Query {
    pub const ALL: [Query; 4] = [
        Query::SystemUnits,
        Query::SystemUnitFiles,
        Query::UserUnits,
        Query::UserUnitFiles,
    ];

    pub fn args(self) -> &'static [&'static str] {
        match self {
            Query::SystemUnits => &[
                "list-units",
                "--type=service",
                "--all",
                "--no-pager",
                "--output=json",
            ],
            Query::SystemUnitFiles => &[
                "list-unit-files",
                "--type=service",
                "--no-pager",
                "--output=json",
            ],
            Query::UserUnits => &[
                "--user",
                "list-units",
                "--type=service",
                "--all",
                "--no-pager",
                "--output=json",
            ],
            Query::UserUnitFiles => &[
                "--user",
                "list-unit-files",
                "--type=service",
                "--no-pager",
                "--output=json",
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    NotInstalled,
    Failed(String),
    TimedOut,
}

/// Find `systemctl` in the usual system directories (never in the current
/// directory, never a relative path).
pub fn locate(path_dirs: &[PathBuf]) -> Option<PathBuf> {
    let standard = [Path::new("/usr/bin"), Path::new("/bin")];
    standard
        .iter()
        .map(|d| d.to_path_buf())
        .chain(path_dirs.iter().filter(|d| d.is_absolute()).cloned())
        .map(|d| d.join(PROGRAM))
        .find(|p| p.is_file())
}

pub fn run(program: &Path, query: Query) -> Result<String, QueryError> {
    let mut command = Command::new(program);
    command
        .args(query.args())
        .env_clear()
        .env("LC_ALL", "C")
        .env("SYSTEMD_PAGER", "")
        .env("SYSTEMD_COLORS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // The user manager is reached through the session's runtime directory.
    for key in ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|e| QueryError::Failed(e.to_string()))?;
    let Some(stdout) = child.stdout.take() else {
        return Err(QueryError::Failed("no output pipe".into()));
    };

    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("sentinel-query".into())
        .spawn(move || {
            let mut text = String::new();
            let result = stdout
                .take(MAX_OUTPUT)
                .read_to_string(&mut text)
                .map(|_| text);
            let _ = tx.send(result);
        });
    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(text)) => {
            let status = child
                .wait()
                .map_err(|e| QueryError::Failed(e.to_string()))?;
            if status.success() {
                Ok(text)
            } else {
                Err(QueryError::Failed(format!("exited with {status}")))
            }
        }
        Ok(Err(e)) => {
            let _ = child.wait();
            Err(QueryError::Failed(e.to_string()))
        }
        Err(_) => {
            // Leave it to finish on its own; collect it so it is no zombie.
            let _ = std::thread::Builder::new()
                .name("sentinel-reaper".into())
                .spawn(move || {
                    let _ = child.wait();
                });
            Err(QueryError::TimedOut)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_query_only_lists() {
        for query in Query::ALL {
            let args = query.args();
            let verbs: Vec<&&str> = args.iter().filter(|a| !a.starts_with("--")).collect();
            assert_eq!(verbs.len(), 1, "{args:?}");
            assert!(
                matches!(*verbs[0], "list-units" | "list-unit-files"),
                "{args:?}"
            );
        }
    }
}
