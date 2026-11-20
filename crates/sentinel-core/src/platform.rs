//! Guards for the environment Sentinel is allowed to run in.

use crate::error::{Result, SentinelError};

/// Real user id of the current process.
pub fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// Sentinel targets Linux. Other systems get a clear refusal instead of
/// half-working behaviour.
pub fn ensure_supported_platform() -> Result<()> {
    ensure_supported_os(std::env::consts::OS)
}

pub fn ensure_supported_os(os: &str) -> Result<()> {
    if os == "linux" {
        Ok(())
    } else {
        Err(SentinelError::UnsupportedPlatform(format!(
            "Sentinel supports Linux only (detected: {os})"
        )))
    }
}

/// Safety policy 3.4: the daemon never runs as root.
pub fn ensure_not_root(uid: u32) -> Result<()> {
    if uid == 0 {
        Err(SentinelError::UnsupportedPlatform(
            "Sentinel must not run as root. Start it from your normal user account".into(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_refused() {
        assert!(matches!(
            ensure_not_root(0),
            Err(SentinelError::UnsupportedPlatform(_))
        ));
        assert!(ensure_not_root(1000).is_ok());
    }

    #[test]
    fn only_linux_is_supported() {
        assert!(ensure_supported_os("linux").is_ok());
        for os in ["macos", "windows", "freebsd"] {
            assert_eq!(ensure_supported_os(os).unwrap_err().exit_code(), 7);
        }
    }
}
