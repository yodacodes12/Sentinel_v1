//! Single-instance protection.
//!
//! The daemon holds an exclusive advisory lock (`flock`) on a lock file for
//! its whole life. The kernel releases the lock when the process dies for
//! any reason, so a crash can never leave Sentinel permanently "running" and
//! there is no PID file to go stale or to be confused by PID reuse.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::error::{Result, SentinelError};

/// Held for the lifetime of the daemon; dropping it releases the lock.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Take the lock, or report [`SentinelError::AlreadyRunning`] if another
    /// daemon holds it.
    pub fn acquire(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|e| SentinelError::io("opening the instance lock", path, e))?;

        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(SentinelError::AlreadyRunning),
            Err(TryLockError::Error(e)) => {
                return Err(SentinelError::io("locking the instance lock", path, e));
            }
        }

        // The PID is informational (for a human inspecting the directory);
        // the lock itself is the source of truth.
        let _ = file
            .set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()))
            .and_then(|()| writeln!(file, "{}", std::process::id()));
        Ok(Self { _file: file })
    }
}

/// Whether some process currently holds the instance lock. Used by clients
/// to tell "daemon is gone" from "daemon is still shutting down".
pub fn is_locked(path: &Path) -> bool {
    match OpenOptions::new().read(true).open(path) {
        Ok(file) => matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock)),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_instance_is_refused_until_the_first_exits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");

        let first = InstanceLock::acquire(&path).unwrap();
        assert!(is_locked(&path));
        let err = InstanceLock::acquire(&path).unwrap_err();
        assert!(matches!(err, SentinelError::AlreadyRunning));
        assert_eq!(err.exit_code(), 10);

        drop(first);
        assert!(!is_locked(&path));
        InstanceLock::acquire(&path).expect("lock is free after the holder is gone");
    }

    #[test]
    fn leftover_lock_file_does_not_block_a_new_instance() {
        // Simulates the file surviving a crash: it exists, with an old PID in
        // it, but nobody holds the lock.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.lock");
        std::fs::write(&path, "999999\n").unwrap();
        assert!(!is_locked(&path));

        let _lock = InstanceLock::acquire(&path).unwrap();
        let pid: u32 = std::fs::read_to_string(&path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(pid, std::process::id());
    }

    #[test]
    fn missing_lock_file_is_not_locked() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_locked(&dir.path().join("absent.lock")));
    }
}
