//! `sentinel logs`: view Sentinel's own log file.
//!
//! Reads exactly one file, the daemon's log under Sentinel's state
//! directory. It has no option to read any other path and never touches the
//! system journal.

use std::fs::File;
use std::io::{self, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

use sentinel_core::{Result, SentinelError};
use serde_json::Value;

/// Only used while the user is actively following the log.
const FOLLOW_INTERVAL: Duration = Duration::from_millis(500);

pub fn run(path: &Path, lines: usize, follow: bool, json: bool) -> Result<ExitCode> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut offset = 0u64;
    match std::fs::read(path) {
        Ok(bytes) => {
            offset = bytes.len() as u64;
            let text = String::from_utf8_lossy(&bytes);
            for line in last_lines(&text, lines) {
                if !emit(&mut out, line, json) {
                    return Ok(ExitCode::SUCCESS);
                }
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            eprintln!(
                "No Sentinel log exists yet ({}). It is created when the daemon first starts.",
                path.display()
            );
            if !follow {
                return Ok(ExitCode::SUCCESS);
            }
        }
        Err(e) => return Err(SentinelError::io("reading the log", path, e)),
    }

    if follow {
        follow_file(path, offset, &mut out, json)?;
    }
    Ok(ExitCode::SUCCESS)
}

/// Print entries appended after `offset` until the reader goes away
/// (Ctrl-C or a closed pipe).
fn follow_file(path: &Path, mut offset: u64, out: &mut impl Write, json: bool) -> Result<()> {
    let mut pending = String::new();
    loop {
        sleep(FOLLOW_INTERVAL);
        let Ok(mut file) = File::open(path) else {
            continue; // not created yet, or mid-rotation
        };
        let length = file.metadata().map(|m| m.len()).unwrap_or(0);
        if length < offset {
            // Rotated or replaced: start again from the top of the new file.
            offset = 0;
            pending.clear();
        }
        if length == offset {
            continue;
        }
        let mut chunk = Vec::new();
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| file.read_to_end(&mut chunk))
            .map_err(|e| SentinelError::io("reading the log", path, e))?;
        offset += chunk.len() as u64;
        pending.push_str(&String::from_utf8_lossy(&chunk));

        // Only complete lines are printed; a partial one waits for its end.
        while let Some(end) = pending.find('\n') {
            let line: String = pending.drain(..=end).collect();
            if !emit(out, line.trim_end(), json) {
                return Ok(());
            }
        }
    }
}

/// Write one entry. Returns false when the output is closed.
fn emit(out: &mut impl Write, line: &str, json: bool) -> bool {
    if line.is_empty() {
        return true;
    }
    let text = if json {
        line.to_string()
    } else {
        format_line(line)
    };
    writeln!(out, "{text}").and_then(|()| out.flush()).is_ok()
}

fn last_lines(text: &str, count: usize) -> impl Iterator<Item = &str> {
    let all: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let skip = all.len().saturating_sub(count);
    all.into_iter().skip(skip)
}

/// Turn one JSON log line into `TIME LEVEL message key=value ...`. Lines
/// that are not JSON objects are shown unchanged.
pub fn format_line(line: &str) -> String {
    let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(line) else {
        return line.to_string();
    };
    let text = |key: &str| fields.get(key).and_then(Value::as_str).unwrap_or("");

    // 2026-11-16T16:25:21.925071Z -> 2026-11-16T16:25:21Z
    let timestamp = text("timestamp");
    let timestamp = match timestamp.split_once('.') {
        Some((seconds, _)) => format!("{seconds}Z"),
        None => timestamp.to_string(),
    };

    let mut out = format!("{timestamp} {:<5} {}", text("level"), text("message"));
    for (key, value) in &fields {
        if matches!(key.as_str(), "timestamp" | "level" | "message" | "target") {
            continue;
        }
        match value {
            Value::String(s) => out.push_str(&format!(" {key}={s}")),
            other => out.push_str(&format!(" {key}={other}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_structured_lines() {
        let line = r#"{"timestamp":"2026-11-16T16:25:21.925071Z","level":"INFO","message":"daemon starting","version":"0.1.0","pid":211483,"target":"sentinel_daemon::run"}"#;
        assert_eq!(
            format_line(line),
            "2026-11-16T16:25:21Z INFO  daemon starting pid=211483 version=0.1.0"
        );
    }

    #[test]
    fn passes_through_unstructured_lines() {
        assert_eq!(format_line("plain text"), "plain text");
        assert_eq!(format_line("[1,2]"), "[1,2]");
    }

    #[test]
    fn tail_returns_the_last_entries_in_order() {
        let text = "a\nb\n\nc\nd\n";
        assert_eq!(last_lines(text, 2).collect::<Vec<_>>(), ["c", "d"]);
        assert_eq!(
            last_lines(text, 99).collect::<Vec<_>>(),
            ["a", "b", "c", "d"]
        );
        assert_eq!(last_lines(text, 0).count(), 0);
    }

    #[test]
    fn emit_reports_closed_output() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert!(!emit(&mut Closed, "x", true));
        let mut buffer = Vec::new();
        assert!(emit(
            &mut buffer,
            "{\"level\":\"WARN\",\"message\":\"m\"}",
            false
        ));
        assert_eq!(String::from_utf8(buffer).unwrap(), " WARN  m\n");
    }

    #[test]
    fn missing_log_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        run(&dir.path().join("daemon.log"), 10, false, false).unwrap();
    }
}
