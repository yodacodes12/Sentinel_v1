//! Structured, local-only logging.
//!
//! The daemon writes one JSON object per line to a private file under the
//! state directory. Writes are synchronous on the calling thread: the daemon
//! logs a handful of lifecycle events, so a background writer thread would be
//! pure overhead.
//!
//! What must never be logged: file contents, credentials, serial numbers,
//! UUIDs or other machine identifiers. Sentinel's own code logs lifecycle
//! events, error categories and its own paths only.

use std::fs::{self, File};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sentinel_core::{Result, SentinelError};
use tracing::Subscriber;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, fmt, registry};

/// A log file larger than this is rotated to `<name>.1` at start-up. One
/// previous file is kept, bounding disk use at roughly twice this size.
pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Environment variable selecting the level (`error`, `warn`, `info`,
/// `debug`, `trace`). Defaults to `info`.
pub const LEVEL_ENV: &str = "SENTINEL_LOG";

#[derive(Debug, Clone)]
pub struct LogOptions {
    /// JSON-lines log file, created with mode `0600`.
    pub file: Option<PathBuf>,
    /// Also write human-readable lines to stderr (foreground runs).
    pub stderr: bool,
    pub level: LevelFilter,
}

impl LogOptions {
    pub fn for_file(file: PathBuf) -> Self {
        Self {
            file: Some(file),
            stderr: false,
            level: level_from_env(),
        }
    }
}

/// Parse a level name; unknown or absent values mean `info`.
pub fn parse_level(value: Option<&str>) -> LevelFilter {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("error") => LevelFilter::ERROR,
        Some("warn" | "warning") => LevelFilter::WARN,
        Some("debug") => LevelFilter::DEBUG,
        Some("trace") => LevelFilter::TRACE,
        Some("off") => LevelFilter::OFF,
        _ => LevelFilter::INFO,
    }
}

pub fn level_from_env() -> LevelFilter {
    parse_level(std::env::var(LEVEL_ENV).ok().as_deref())
}

/// Build a subscriber without installing it. Tests scope it with
/// `tracing::subscriber::with_default`.
pub fn build(options: &LogOptions) -> Result<impl Subscriber + Send + Sync + 'static> {
    let file_layer = match &options.file {
        Some(path) => Some(
            fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .with_span_list(false)
                .with_writer(Mutex::new(open_log_file(path)?)),
        ),
        None => None,
    };
    let stderr_layer = options.stderr.then(|| {
        fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .with_writer(std::io::stderr)
    });
    // `Layer::and_then` is spelled out because `Option` has its own `and_then`.
    Ok(registry().with(Layer::and_then(file_layer, stderr_layer).with_filter(options.level)))
}

/// Install the process-wide subscriber. Fails if one is already installed.
pub fn init(options: &LogOptions) -> Result<()> {
    build(options)?
        .try_init()
        .map_err(|e| SentinelError::Logging(e.to_string()))
}

/// Open (creating if needed) a private, append-only log file, rotating it
/// first when it has grown past [`MAX_LOG_BYTES`].
pub fn open_log_file(path: &Path) -> Result<File> {
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > MAX_LOG_BYTES {
            fs::rename(path, rotated_path(path))
                .map_err(|e| SentinelError::io("rotating log file", path, e))?;
        } else if meta.permissions().mode() & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|e| SentinelError::io("restricting log file", path, e))?;
        }
    }
    fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| SentinelError::io("opening log file", path, e))
}

/// Where the previous log generation is kept.
pub fn rotated_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".1");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn level_parsing() {
        assert_eq!(parse_level(None), LevelFilter::INFO);
        assert_eq!(parse_level(Some("DEBUG")), LevelFilter::DEBUG);
        assert_eq!(parse_level(Some(" warn ")), LevelFilter::WARN);
        assert_eq!(parse_level(Some("nonsense")), LevelFilter::INFO);
    }

    #[test]
    fn writes_structured_json_lines_to_a_private_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.log");
        let subscriber = build(&LogOptions {
            file: Some(path.clone()),
            stderr: false,
            level: LevelFilter::INFO,
        })
        .unwrap();

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(pid = 42, "daemon started");
            tracing::debug!("filtered out");
            tracing::warn!(reason = "test", "something to note");
        });

        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("each line is JSON"))
            .collect();
        assert_eq!(lines.len(), 2, "debug line must be filtered: {text}");
        assert_eq!(lines[0]["level"], "INFO");
        assert_eq!(lines[0]["message"], "daemon started");
        assert_eq!(lines[0]["pid"], 42);
        assert!(lines[0]["timestamp"].is_string());
        assert_eq!(lines[1]["level"], "WARN");

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn appends_rather_than_truncates() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.log");
        fs::write(&path, "existing\n").unwrap();
        let mut file = open_log_file(&path).unwrap();
        file.write_all(b"new\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "existing\nnew\n");
    }

    #[test]
    fn tightens_permissions_on_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.log");
        fs::write(&path, "x").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        open_log_file(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn oversized_log_is_rotated() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("daemon.log");
        let file = File::create(&path).unwrap();
        file.set_len(MAX_LOG_BYTES + 1).unwrap();
        drop(file);

        open_log_file(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), 0);
        assert_eq!(
            fs::metadata(rotated_path(&path)).unwrap().len(),
            MAX_LOG_BYTES + 1
        );
    }

    #[test]
    fn unwritable_location_is_a_clear_error() {
        let err = open_log_file(Path::new("/proc/sentinel-cannot-exist/daemon.log")).unwrap_err();
        assert!(matches!(
            err,
            SentinelError::Io { .. } | SentinelError::PermissionDenied { .. }
        ));
    }

    #[test]
    fn global_init_succeeds_once_then_reports_the_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let options = LogOptions {
            file: Some(tmp.path().join("daemon.log")),
            stderr: false,
            level: LevelFilter::INFO,
        };
        init(&options).unwrap();
        assert!(matches!(init(&options), Err(SentinelError::Logging(_))));
    }
}
