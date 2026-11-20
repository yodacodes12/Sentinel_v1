//! Blocking client used by the CLI and the desktop shell.
//!
//! It is synchronous on purpose: clients make a handful of short local
//! requests, so they need neither an async runtime nor extra threads.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use sentinel_config::{ConfigPatch, SentinelConfig};
use sentinel_core::{MonitoringCommand, Result, SentinelError};
use sentinel_inventory::{DiscoveryJob, DiscoveryStatus, HardwareCapabilities};
use sentinel_telemetry::model::{Alert, HistoryQuery, MonitoringReport, SamplingInfo, Series};

use crate::codec::{decode_response, encode};
use crate::protocol::{
    DaemonStatus, InventoryAnswer, MAX_RESPONSE_BYTES, MetricsAnswer, MonitoringStatus, Outcome,
    ProcessAnswer, Request, RequestEnvelope, Response, UtilitiesAnswer,
};

/// How long a client waits for the daemon to answer one request.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

pub struct Client {
    reader: BufReader<UnixStream>,
    next_id: u64,
}

impl Client {
    /// Connect to the daemon's socket.
    ///
    /// A missing socket or a refused connection both mean "no daemon is
    /// running" and are reported as [`SentinelError::DaemonUnavailable`].
    pub fn connect(socket: &Path) -> Result<Self> {
        Self::connect_with_timeout(socket, DEFAULT_TIMEOUT)
    }

    pub fn connect_with_timeout(socket: &Path, timeout: Duration) -> Result<Self> {
        let stream = UnixStream::connect(socket).map_err(|e| match e.kind() {
            ErrorKind::NotFound | ErrorKind::ConnectionRefused => SentinelError::DaemonUnavailable,
            ErrorKind::PermissionDenied => SentinelError::PermissionDenied {
                path: socket.to_path_buf(),
                source: e,
            },
            _ => SentinelError::Ipc(format!("cannot connect to {}: {e}", socket.display())),
        })?;
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .map_err(|e| SentinelError::Ipc(format!("cannot configure the connection: {e}")))?;
        Ok(Self {
            reader: BufReader::new(stream),
            next_id: 1,
        })
    }

    /// Send one request and wait for its response.
    pub fn call(&mut self, request: Request) -> Result<Response> {
        let id = self.next_id;
        self.next_id += 1;

        let frame = encode(&RequestEnvelope::new(id, request));
        self.reader
            .get_mut()
            .write_all(&frame)
            .map_err(|e| transport_error("sending the request", e))?;

        let mut line = Vec::new();
        let read = (&mut self.reader)
            .take(MAX_RESPONSE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(|e| transport_error("waiting for the daemon's answer", e))?;
        if read == 0 {
            return Err(SentinelError::Ipc(
                "the daemon closed the connection without answering".into(),
            ));
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        } else {
            return Err(SentinelError::Protocol(
                "the daemon's answer was truncated or too large".into(),
            ));
        }

        let envelope = decode_response(&line).map_err(SentinelError::Protocol)?;
        match envelope.outcome {
            // An error may carry no id when the daemon could not read ours.
            Outcome::Error { error } => Err(SentinelError::Daemon {
                code: error.code.as_str().to_string(),
                message: error.message,
            }),
            Outcome::Ok { .. } if envelope.id != Some(id) => Err(SentinelError::Protocol(format!(
                "answer id {:?} does not match request id {id}",
                envelope.id
            ))),
            Outcome::Ok { response } => Ok(*response),
        }
    }

    pub fn status(&mut self) -> Result<DaemonStatus> {
        match self.call(Request::GetDaemonStatus)? {
            Response::DaemonStatus(status) => Ok(status),
            other => Err(unexpected("DaemonStatus", &other)),
        }
    }

    /// Returns `(application version, protocol version)`.
    pub fn application_version(&mut self) -> Result<(String, u32)> {
        match self.call(Request::GetApplicationVersion)? {
            Response::ApplicationVersion {
                app_version,
                protocol_version,
            } => Ok((app_version, protocol_version)),
            other => Err(unexpected("ApplicationVersion", &other)),
        }
    }

    /// Returns the configuration and the path of the file backing it.
    pub fn configuration(&mut self) -> Result<(SentinelConfig, String)> {
        match self.call(Request::GetConfiguration)? {
            Response::Configuration { config, path } => Ok((config, path)),
            other => Err(unexpected("Configuration", &other)),
        }
    }

    pub fn update_configuration(&mut self, patch: ConfigPatch) -> Result<(SentinelConfig, String)> {
        match self.call(Request::UpdateConfiguration { patch })? {
            Response::Configuration { config, path } => Ok((config, path)),
            other => Err(unexpected("Configuration", &other)),
        }
    }

    pub fn monitoring(&mut self, command: MonitoringCommand) -> Result<MonitoringStatus> {
        match self.call(Request::from_monitoring_command(command))? {
            Response::Monitoring(status) => Ok(status),
            other => Err(unexpected("Monitoring", &other)),
        }
    }

    pub fn discovery_status(&mut self) -> Result<DiscoveryStatus> {
        match self.call(Request::GetDiscoveryStatus)? {
            Response::DiscoveryStatus(status) => Ok(status),
            other => Err(unexpected("DiscoveryStatus", &other)),
        }
    }

    /// Start (or, with `force`, repeat) a scan. Returns progress at once;
    /// the scan runs in the daemon.
    pub fn run_discovery(&mut self, job: DiscoveryJob, force: bool) -> Result<DiscoveryStatus> {
        let request = match (job, force) {
            (DiscoveryJob::Hardware, false) => Request::RunHardwareDiscovery,
            (DiscoveryJob::Hardware, true) => Request::RefreshHardwareInventory,
            (DiscoveryJob::Utilities, false) => Request::DiscoverSystemUtilities,
            (DiscoveryJob::Utilities, true) => Request::RefreshSystemUtilities,
        };
        match self.call(request)? {
            Response::DiscoveryStatus(status) => Ok(status),
            other => Err(unexpected("DiscoveryStatus", &other)),
        }
    }

    pub fn cancel_discovery(&mut self, job: DiscoveryJob) -> Result<DiscoveryStatus> {
        match self.call(Request::CancelDiscovery { job })? {
            Response::DiscoveryStatus(status) => Ok(status),
            other => Err(unexpected("DiscoveryStatus", &other)),
        }
    }

    pub fn hardware_inventory(&mut self) -> Result<InventoryAnswer> {
        match self.call(Request::GetHardwareInventory)? {
            Response::HardwareInventory(answer) => Ok(answer),
            other => Err(unexpected("HardwareInventory", &other)),
        }
    }

    pub fn system_utilities(&mut self) -> Result<UtilitiesAnswer> {
        match self.call(Request::GetSystemUtilities)? {
            Response::SystemUtilities(answer) => Ok(answer),
            other => Err(unexpected("SystemUtilities", &other)),
        }
    }

    pub fn hardware_capabilities(&mut self) -> Result<HardwareCapabilities> {
        match self.call(Request::GetHardwareCapabilities)? {
            Response::HardwareCapabilities(caps) => Ok(caps),
            other => Err(unexpected("HardwareCapabilities", &other)),
        }
    }

    pub fn monitoring_report(&mut self) -> Result<MonitoringReport> {
        match self.call(Request::GetMonitoringStatus)? {
            Response::MonitoringReport(r) => Ok(*r),
            other => Err(unexpected("MonitoringReport", &other)),
        }
    }

    /// Latest telemetry. `request` must be one of the `Get*Metrics`
    /// requests other than processes.
    pub fn metrics(&mut self, request: Request) -> Result<MetricsAnswer> {
        match self.call(request)? {
            Response::Metrics(m) => Ok(*m),
            other => Err(unexpected("Metrics", &other)),
        }
    }

    pub fn processes(&mut self) -> Result<ProcessAnswer> {
        match self.call(Request::GetProcessMetrics)? {
            Response::Processes(p) => Ok(*p),
            other => Err(unexpected("Processes", &other)),
        }
    }

    pub fn alerts(&mut self, request: Request) -> Result<Vec<Alert>> {
        match self.call(request)? {
            Response::Alerts { alerts } => Ok(alerts),
            other => Err(unexpected("Alerts", &other)),
        }
    }

    pub fn sampling(&mut self, request: Request) -> Result<SamplingInfo> {
        match self.call(request)? {
            Response::Sampling(s) => Ok(s),
            other => Err(unexpected("Sampling", &other)),
        }
    }

    pub fn history(&mut self, query: HistoryQuery) -> Result<Vec<Series>> {
        match self.call(Request::GetTelemetryHistory { query })? {
            Response::History { series } => Ok(series),
            other => Err(unexpected("History", &other)),
        }
    }

    /// Any document-discovery request; all answer with the documents view.
    pub fn documents(&mut self, request: Request) -> Result<sentinel_docs::DocumentsView> {
        match self.call(request)? {
            Response::Documents(d) => Ok(*d),
            other => Err(unexpected("Documents", &other)),
        }
    }

    /// Ask the daemon to shut down. Success means the request was accepted,
    /// not that the process has already exited.
    pub fn shutdown(&mut self) -> Result<()> {
        match self.call(Request::Shutdown)? {
            Response::ShutdownAccepted => Ok(()),
            other => Err(unexpected("ShutdownAccepted", &other)),
        }
    }
}

fn transport_error(doing: &str, error: std::io::Error) -> SentinelError {
    match error.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut => SentinelError::Timeout(doing.to_string()),
        _ => SentinelError::Ipc(format!("{doing} failed: {error}")),
    }
}

fn unexpected(expected: &str, got: &Response) -> SentinelError {
    let got = match got {
        Response::DaemonStatus(_) => "DaemonStatus",
        Response::Monitoring(_) => "Monitoring",
        Response::ApplicationVersion { .. } => "ApplicationVersion",
        Response::Configuration { .. } => "Configuration",
        Response::ShutdownAccepted => "ShutdownAccepted",
        Response::DiscoveryStatus(_) => "DiscoveryStatus",
        Response::HardwareInventory(_) => "HardwareInventory",
        Response::SystemUtilities(_) => "SystemUtilities",
        Response::HardwareCapabilities(_) => "HardwareCapabilities",
        Response::MonitoringReport(_) => "MonitoringReport",
        Response::Metrics(_) => "Metrics",
        Response::Processes(_) => "Processes",
        Response::Alerts { .. } => "Alerts",
        Response::Sampling(_) => "Sampling",
        Response::History { .. } => "History",
        Response::Documents(_) => "Documents",
    };
    SentinelError::Protocol(format!("expected a {expected} answer, got {got}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ErrorCode, IpcError, ResponseEnvelope};
    use std::os::unix::net::UnixListener;
    use std::thread;

    /// A one-connection fake daemon that answers each line with `answer`.
    fn fake_daemon(
        answer: impl Fn(&str) -> Option<Vec<u8>> + Send + 'static,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.sock");
        let listener = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                match answer(line.trim_end()) {
                    Some(bytes) => reader.get_mut().write_all(&bytes).unwrap(),
                    None => break,
                }
                line.clear();
            }
        });
        (dir, path)
    }

    fn request_id(line: &str) -> u64 {
        serde_json::from_str::<RequestEnvelope>(line).unwrap().id
    }

    #[test]
    fn missing_socket_means_daemon_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let err = Client::connect(&dir.path().join("nope.sock"))
            .err()
            .unwrap();
        assert!(matches!(err, SentinelError::DaemonUnavailable));
    }

    #[test]
    fn stale_socket_means_daemon_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.sock");
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists(), "the socket file outlives its listener");
        let err = Client::connect(&path).err().unwrap();
        assert!(matches!(err, SentinelError::DaemonUnavailable));
    }

    #[test]
    fn successful_call_returns_the_typed_response() {
        let (_dir, path) = fake_daemon(|line| {
            Some(encode(&ResponseEnvelope::new(
                Some(request_id(line)),
                Ok(Response::ApplicationVersion {
                    app_version: "9.9.9".into(),
                    protocol_version: 1,
                }),
            )))
        });
        let mut client = Client::connect(&path).unwrap();
        assert_eq!(client.application_version().unwrap(), ("9.9.9".into(), 1));
        // Ids increase and are matched on every call.
        assert_eq!(client.application_version().unwrap().0, "9.9.9");
    }

    #[test]
    fn daemon_errors_surface_with_code_and_message() {
        let (_dir, path) = fake_daemon(|line| {
            Some(encode(&ResponseEnvelope::new(
                Some(request_id(line)),
                Err(IpcError::new(ErrorCode::MonitoringUnavailable, "no engine")),
            )))
        });
        let err = Client::connect(&path)
            .unwrap()
            .monitoring(MonitoringCommand::Start)
            .unwrap_err();
        match err {
            SentinelError::Daemon { code, message } => {
                assert_eq!(code, "monitoring_unavailable");
                assert_eq!(message, "no engine");
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn mismatched_id_and_wrong_variant_are_protocol_errors() {
        let (_dir, path) = fake_daemon(|_| {
            Some(encode(&ResponseEnvelope::new(
                Some(999),
                Ok(Response::ShutdownAccepted),
            )))
        });
        let err = Client::connect(&path).unwrap().shutdown().unwrap_err();
        assert!(matches!(err, SentinelError::Protocol(_)));

        let (_dir, path) = fake_daemon(|line| {
            Some(encode(&ResponseEnvelope::new(
                Some(request_id(line)),
                Ok(Response::ShutdownAccepted),
            )))
        });
        let err = Client::connect(&path).unwrap().status().unwrap_err();
        assert!(matches!(err, SentinelError::Protocol(_)));
    }

    #[test]
    fn garbage_and_disconnects_are_handled() {
        let (_dir, path) = fake_daemon(|_| Some(b"%%% not json %%%\n".to_vec()));
        let err = Client::connect(&path).unwrap().status().unwrap_err();
        assert!(matches!(err, SentinelError::Protocol(_)));

        let (_dir, path) = fake_daemon(|_| None);
        let err = Client::connect(&path).unwrap().status().unwrap_err();
        assert!(matches!(err, SentinelError::Ipc(_)));
    }

    #[test]
    fn silent_daemon_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let mut client = Client::connect_with_timeout(&path, Duration::from_millis(150)).unwrap();
        let err = client.status().unwrap_err();
        assert!(matches!(err, SentinelError::Timeout(_)), "{err:?}");
        assert_eq!(err.exit_code(), 9);
        drop(listener);
    }
}
