//! Locating and launching Sentinel's own binaries.
//!
//! Apart from the fixed read-only systemd queries in `sentinel-utilities`,
//! this is the only place that starts a process. It starts Sentinel's own
//! binaries, and the desktop environment's application launcher when the
//! user asks to open an installed utility. Arguments are passed as an
//! argument vector; no shell is ever involved.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use sentinel_core::{Result, SentinelError};

pub const DAEMON_BINARY: &str = "sentineld";
pub const DAEMON_ENV: &str = "SENTINEL_DAEMON_BIN";
pub const DESKTOP_BINARY: &str = "sentinel-desktop";
pub const DESKTOP_ENV: &str = "SENTINEL_DESKTOP_BIN";

/// GLib's launcher: `gio launch <file.desktop>` starts an application the
/// same way the desktop's own menu does.
pub const GIO_BINARY: &str = "gio";
/// GTK's launcher, used when `gio` is missing: `gtk-launch <desktop id>`.
pub const GTK_LAUNCH_BINARY: &str = "gtk-launch";

/// Find a desktop launcher on `PATH`. Only the two names above are accepted.
pub fn locate_desktop_launcher(name: &str) -> Result<PathBuf> {
    if name != GIO_BINARY && name != GTK_LAUNCH_BINARY {
        return Err(SentinelError::Launch {
            what: name.into(),
            reason: "not a desktop launcher Sentinel uses".into(),
        });
    }
    locate_with(name, None, None, std::env::var_os("PATH")).map_err(|reason| {
        SentinelError::Launch {
            what: name.into(),
            reason,
        }
    })
}

/// Check that `path` is a desktop entry file in an `applications`
/// directory: absolute, no `..`, a regular file ending in `.desktop`.
pub fn validate_desktop_entry(path: &Path) -> std::result::Result<(), String> {
    use std::path::Component;
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("the desktop entry path is not a plain absolute path".into());
    }
    if path.extension().is_none_or(|e| e != "desktop") {
        return Err("not a desktop entry file".into());
    }
    if !path
        .ancestors()
        .any(|a| a.file_name().is_some_and(|n| n == "applications"))
    {
        return Err("the desktop entry is not inside an applications directory".into());
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() || meta.file_type().is_symlink() => Ok(()),
        Ok(_) => Err("the desktop entry is not a file".into()),
        Err(_) => Err("the desktop entry no longer exists; scan for utilities again".into()),
    }
}

/// Open an application from its desktop entry, as the user's menu would.
/// Waits briefly so an immediate failure is reported instead of claiming
/// success.
pub fn open_desktop_entry(path: &Path, desktop_id: &str) -> Result<()> {
    validate_desktop_entry(path).map_err(|reason| SentinelError::Launch {
        what: desktop_id.into(),
        reason,
    })?;
    let path_text = path.to_str().ok_or_else(|| SentinelError::Launch {
        what: desktop_id.into(),
        reason: "the desktop entry path is not valid UTF-8".into(),
    })?;
    let mut child = match locate_desktop_launcher(GIO_BINARY) {
        Ok(gio) => spawn_detached(&gio, &["launch", path_text])?,
        Err(_) => {
            let gtk = locate_desktop_launcher(GTK_LAUNCH_BINARY).map_err(|_| SentinelError::Launch {
                what: desktop_id.into(),
                reason: "neither `gio` nor `gtk-launch` is installed, so Sentinel cannot open applications".into(),
            })?;
            let id = desktop_id.trim_end_matches(".desktop");
            spawn_detached(&gtk, &[id])?
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(SentinelError::Launch {
                    what: desktop_id.into(),
                    reason: format!("the desktop launcher reported failure ({status})"),
                });
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                // Still running: the launcher is waiting on the application.
                reap_in_background(child);
                return Ok(());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
            Err(e) => {
                return Err(SentinelError::Launch {
                    what: desktop_id.into(),
                    reason: e.to_string(),
                });
            }
        }
    }
}

/// Open a web page or a document with the user's default application, via
/// `gio open`. The caller validates `target` first (an https:// address or
/// an absolute path to a checked file); it is passed after `--`, so it can
/// never be read as an option. Nothing else is ever opened this way.
pub fn open_with_default_application(target: &str) -> Result<()> {
    if !(target.starts_with("https://") || target.starts_with('/'))
        || target.chars().any(char::is_control)
    {
        return Err(SentinelError::Launch {
            what: target.chars().take(80).collect(),
            reason: "only https addresses and checked local documents can be opened".into(),
        });
    }
    let gio = locate_desktop_launcher(GIO_BINARY).map_err(|_| SentinelError::Launch {
        what: "document".into(),
        reason: "`gio` is not installed, so Sentinel cannot open documents or web pages".into(),
    })?;
    let child = spawn_detached(&gio, &["open", "--", target])?;
    reap_in_background(child);
    Ok(())
}

/// Find a Sentinel binary: an explicit override, then next to the current
/// executable (the normal installed and development layout), then `PATH`.
pub fn locate(name: &str, env_override: &str) -> Result<PathBuf> {
    locate_with(
        name,
        std::env::var_os(env_override),
        std::env::current_exe().ok(),
        std::env::var_os("PATH"),
    )
    .map_err(|reason| SentinelError::Launch {
        what: name.to_string(),
        reason,
    })
}

fn locate_with(
    name: &str,
    override_path: Option<OsString>,
    current_exe: Option<PathBuf>,
    path_var: Option<OsString>,
) -> std::result::Result<PathBuf, String> {
    if let Some(explicit) = override_path.filter(|v| !v.is_empty()) {
        let explicit = PathBuf::from(explicit);
        return if is_executable(&explicit) {
            Ok(explicit)
        } else {
            Err(format!("{} is not an executable file", explicit.display()))
        };
    }

    let beside = current_exe
        .as_deref()
        .and_then(Path::parent)
        .map(|dir| dir.join(name));
    let on_path = path_var
        .iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name));

    beside
        .into_iter()
        .chain(on_path)
        .find(|candidate| is_executable(candidate))
        .ok_or_else(|| format!("`{name}` was not found next to this program or on PATH"))
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Start `binary` detached from the caller: no inherited terminal I/O and
/// its own process group, so Ctrl-C in the launching terminal does not
/// reach it. It runs as the same user with the same environment.
pub fn spawn_detached(binary: &Path, args: &[&str]) -> Result<Child> {
    Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| SentinelError::Launch {
            what: binary.display().to_string(),
            reason: e.to_string(),
        })
}

/// Collect the child's exit status whenever it comes, so a long-lived
/// parent (the desktop app) does not accumulate zombie processes. The
/// thread is blocked in `wait` and costs nothing while the child runs.
pub fn reap_in_background(mut child: Child) {
    let _ = std::thread::Builder::new()
        .name("sentinel-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn executable(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn override_wins_and_must_be_executable() {
        let dir = tempfile::tempdir().unwrap();
        let bin = executable(dir.path(), "custom");
        assert_eq!(
            locate_with("sentineld", Some(bin.clone().into()), None, None).unwrap(),
            bin
        );

        let plain = dir.path().join("plain");
        fs::write(&plain, "x").unwrap();
        assert!(locate_with("sentineld", Some(plain.into()), None, None).is_err());
        assert!(
            locate_with(
                "sentineld",
                Some("/nonexistent/sentineld".into()),
                None,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn sibling_of_current_executable_is_preferred_over_path() {
        let beside = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let sibling = executable(beside.path(), "sentineld");
        executable(elsewhere.path(), "sentineld");

        let found = locate_with(
            "sentineld",
            None,
            Some(beside.path().join("sentinel")),
            Some(elsewhere.path().into()),
        )
        .unwrap();
        assert_eq!(found, sibling);
    }

    #[test]
    fn falls_back_to_path_and_skips_relative_entries() {
        let dir = tempfile::tempdir().unwrap();
        let bin = executable(dir.path(), "sentineld");
        let path_var = std::env::join_paths([Path::new("relative/bin"), dir.path()]).unwrap();
        assert_eq!(
            locate_with("sentineld", None, None, Some(path_var)).unwrap(),
            bin
        );
    }

    #[test]
    fn missing_binary_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = locate_with("sentineld", None, None, Some(dir.path().into())).unwrap_err();
        assert!(err.contains("sentineld"));
    }

    #[test]
    fn desktop_entry_validation() {
        let dir = tempfile::tempdir().unwrap();
        let apps = dir.path().join("share/applications");
        fs::create_dir_all(&apps).unwrap();
        let entry = apps.join("htop.desktop");
        fs::write(&entry, "[Desktop Entry]\n").unwrap();
        assert!(validate_desktop_entry(&entry).is_ok());
        assert!(validate_desktop_entry(Path::new("relative/applications/x.desktop")).is_err());
        assert!(validate_desktop_entry(&apps.join("../applications/htop.desktop")).is_err());
        let script = apps.join("evil.sh");
        fs::write(&script, "").unwrap();
        assert!(validate_desktop_entry(&script).is_err());
        let outside = dir.path().join("x.desktop");
        fs::write(&outside, "").unwrap();
        assert!(validate_desktop_entry(&outside).is_err());
        assert!(validate_desktop_entry(&apps.join("gone.desktop")).is_err());
        assert!(locate_desktop_launcher("sh").is_err());
    }

    #[test]
    fn spawn_failure_is_reported() {
        let err = spawn_detached(Path::new("/nonexistent/sentineld"), &[]).unwrap_err();
        assert!(matches!(err, SentinelError::Launch { .. }));
        assert_eq!(err.exit_code(), 8);
    }
}
