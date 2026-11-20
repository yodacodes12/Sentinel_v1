//! Creation and cleanup of the IPC socket file.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};

use sentinel_core::{Result, SentinelError};
use tokio::net::UnixListener;

/// A bound listener that removes its socket file when dropped.
pub struct BoundSocket {
    listener: Option<UnixListener>,
    path: PathBuf,
}

impl BoundSocket {
    /// Bind the daemon's socket at `path`, clearing a stale socket first.
    ///
    /// Must be called while holding the instance lock: that is what makes
    /// it safe to conclude that an existing socket file has no live owner.
    /// Must be called from within a Tokio runtime.
    pub fn bind(path: &Path) -> Result<Self> {
        remove_stale_socket(path)?;
        let listener = UnixListener::bind(path)
            .map_err(|e| SentinelError::io("binding the IPC socket", path, e))?;
        // The parent directory is already 0700; this is a second barrier.
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| SentinelError::io("restricting the IPC socket", path, e))?;
        Ok(Self {
            listener: Some(listener),
            path: path.to_path_buf(),
        })
    }

    pub fn listener(&self) -> &UnixListener {
        self.listener.as_ref().expect("listener present until drop")
    }
}

impl Drop for BoundSocket {
    fn drop(&mut self) {
        // Close first so no connection is accepted on a vanishing path.
        self.listener.take();
        let _ = fs::remove_file(&self.path);
    }
}

/// Remove a leftover socket file. Anything that is not a socket is left
/// alone and reported: Sentinel does not delete files it does not recognise.
pub fn remove_stale_socket(path: &Path) -> Result<()> {
    // symlink_metadata: never follow a link and delete its target's namesake.
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SentinelError::io("inspecting the IPC socket", path, e)),
        Ok(meta) if meta.file_type().is_socket() => {
            tracing::info!("removing stale IPC socket left by a previous run");
            fs::remove_file(path).map_err(|e| SentinelError::io("removing stale socket", path, e))
        }
        Ok(_) => Err(SentinelError::Ipc(format!(
            "{} exists but is not a socket; Sentinel will not remove it. Move it away and \
             start Sentinel again",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener as StdListener;

    #[test]
    fn stale_socket_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.sock");
        drop(StdListener::bind(&path).unwrap());
        assert!(path.exists());
        remove_stale_socket(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn absent_socket_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        remove_stale_socket(&dir.path().join("daemon.sock")).unwrap();
    }

    #[test]
    fn regular_file_is_refused_and_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.sock");
        fs::write(&path, "precious").unwrap();
        assert!(matches!(
            remove_stale_socket(&path),
            Err(SentinelError::Ipc(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "precious");
    }

    #[test]
    fn symlink_is_refused_and_its_target_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.sock");
        let _listener = StdListener::bind(&target).unwrap();
        let link = dir.path().join("daemon.sock");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(remove_stale_socket(&link).is_err());
        assert!(target.exists());
    }

    #[tokio::test]
    async fn bind_creates_a_private_socket_and_drop_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.sock");
        drop(StdListener::bind(&path).unwrap()); // stale leftover

        let bound = BoundSocket::bind(&path).unwrap();
        let meta = fs::symlink_metadata(&path).unwrap();
        assert!(meta.file_type().is_socket());
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);

        drop(bound);
        assert!(!path.exists());
    }
}
