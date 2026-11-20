//! Encoding and decoding of frames, shared by the daemon and the client so
//! both sides apply identical rules.

use serde::Serialize;
use serde_json::Value;

use crate::protocol::{
    ErrorCode, IpcError, MAX_FRAME_BYTES, MAX_RESPONSE_BYTES, PROTOCOL_VERSION, RequestEnvelope,
    ResponseEnvelope,
};

/// Why an incoming request frame was rejected, with the request id when it
/// could still be read so the client can correlate the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeFailure {
    pub id: Option<u64>,
    pub error: IpcError,
}

impl DecodeFailure {
    fn new(id: Option<u64>, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            id,
            error: IpcError::new(code, message),
        }
    }
}

/// Serialise a message as one newline-terminated line.
pub fn encode<T: Serialize>(message: &T) -> Vec<u8> {
    // serde_json never emits a raw newline, so the frame is always exactly
    // one line. Should a value ever fail to serialise, the peer gets an
    // internal error instead of the connection being dropped.
    let mut bytes = serde_json::to_vec(message).unwrap_or_else(|_| {
        br#"{"version":1,"id":null,"outcome":{"status":"error","error":{"code":"internal","message":"the answer could not be encoded"}}}"#.to_vec()
    });
    bytes.push(b'\n');
    bytes
}

/// Decode and validate one request frame (without its trailing newline).
///
/// Checks are ordered so the most useful error wins: size, JSON shape,
/// protocol version, identifier, request type, then request-specific rules.
pub fn decode_request(frame: &[u8]) -> Result<RequestEnvelope, DecodeFailure> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(DecodeFailure::new(
            None,
            ErrorCode::FrameTooLarge,
            format!("message exceeds {MAX_FRAME_BYTES} bytes"),
        ));
    }
    let value: Value = serde_json::from_slice(frame).map_err(|_| {
        DecodeFailure::new(
            None,
            ErrorCode::MalformedRequest,
            "message is not valid JSON",
        )
    })?;
    let Some(object) = value.as_object() else {
        return Err(DecodeFailure::new(
            None,
            ErrorCode::MalformedRequest,
            "message must be a JSON object",
        ));
    };

    let id = object.get("id").and_then(Value::as_u64);

    match object.get("version").and_then(Value::as_u64) {
        None => {
            return Err(DecodeFailure::new(
                id,
                ErrorCode::MalformedRequest,
                "message has no numeric `version`",
            ));
        }
        Some(version) if version != u64::from(PROTOCOL_VERSION) => {
            return Err(DecodeFailure::new(
                id,
                ErrorCode::UnsupportedVersion,
                format!(
                    "protocol version {version} is not supported; this daemon speaks version \
                     {PROTOCOL_VERSION}"
                ),
            ));
        }
        Some(_) => {}
    }

    if id.is_none() {
        return Err(DecodeFailure::new(
            None,
            ErrorCode::MalformedRequest,
            "message has no numeric `id`",
        ));
    }

    let request_keys: Option<Vec<String>> = object
        .get("request")
        .and_then(Value::as_object)
        .map(|r| r.keys().cloned().collect());
    let envelope: RequestEnvelope = serde_json::from_value(value).map_err(|e| {
        DecodeFailure::new(
            id,
            ErrorCode::InvalidRequest,
            format!("unrecognised or invalid request: {e}"),
        )
    })?;
    // Serde ignores extra fields on requests without a payload; nothing may
    // ride along unnoticed, so any key the typed request lacks is an error.
    if let Some(keys) = request_keys {
        let canonical = serde_json::to_value(&envelope.request).unwrap_or(Value::Null);
        if let Some(extra) = keys.iter().find(|k| canonical.get(k.as_str()).is_none()) {
            return Err(DecodeFailure::new(
                id,
                ErrorCode::InvalidRequest,
                format!("unexpected field `{extra}` in request"),
            ));
        }
    }
    envelope
        .request
        .validate()
        .map_err(|error| DecodeFailure { id, error })?;
    Ok(envelope)
}

/// Decode a response frame on the client side.
pub fn decode_response(frame: &[u8]) -> Result<ResponseEnvelope, String> {
    if frame.len() > MAX_RESPONSE_BYTES {
        return Err(format!("response exceeds {MAX_RESPONSE_BYTES} bytes"));
    }
    let envelope: ResponseEnvelope =
        serde_json::from_slice(frame).map_err(|e| format!("unreadable response: {e}"))?;
    if envelope.version != PROTOCOL_VERSION {
        return Err(format!(
            "daemon answered with protocol version {}, expected {PROTOCOL_VERSION}",
            envelope.version
        ));
    }
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{DaemonStatus, MonitoringStatus, Outcome, Request, Response};
    use sentinel_config::{ConfigPatch, PowerMode, SentinelConfig, Theme};
    use sentinel_core::{DaemonState, MonitoringState};

    fn all_requests() -> Vec<Request> {
        vec![
            Request::GetDaemonStatus,
            Request::StartMonitoring,
            Request::StopMonitoring,
            Request::PauseMonitoring,
            Request::ResumeMonitoring,
            Request::GetApplicationVersion,
            Request::GetConfiguration,
            Request::UpdateConfiguration {
                patch: ConfigPatch {
                    theme: Some(Theme::Black),
                    ..Default::default()
                },
            },
            Request::Shutdown,
            Request::RunHardwareDiscovery,
            Request::RefreshHardwareInventory,
            Request::GetHardwareInventory,
            Request::GetDiscoveryStatus,
            Request::DiscoverSystemUtilities,
            Request::RefreshSystemUtilities,
            Request::GetSystemUtilities,
            Request::GetHardwareCapabilities,
            Request::CancelDiscovery {
                job: sentinel_inventory::DiscoveryJob::Hardware,
            },
            Request::GetMonitoringStatus,
            Request::GetLatestMetrics,
            Request::GetProcessMetrics,
            Request::GetAlertHistory { limit: 50 },
            Request::AcknowledgeAlert { alert_id: 7 },
            Request::UpdateSamplingProfile {
                profile: sentinel_telemetry::model::SamplingProfile::Eco,
                power_aware: Some(true),
            },
            Request::GetTelemetryHistory {
                query: sentinel_telemetry::model::HistoryQuery {
                    series: vec![sentinel_telemetry::model::SeriesKey::new("cpu.total").unwrap()],
                    window: sentinel_telemetry::model::TimeWindow::Hour1,
                },
            },
        ]
    }

    fn sample_status() -> DaemonStatus {
        DaemonStatus {
            app_version: "0.1.0".into(),
            protocol_version: PROTOCOL_VERSION,
            pid: 4242,
            state: DaemonState::Running,
            started_at: 1_700_000_000,
            uptime_secs: 12,
            monitoring: MonitoringStatus {
                state: MonitoringState::Stopped,
                engine_available: false,
                note: "not available".into(),
            },
            operating_mode: PowerMode::Balanced,
            config_path: "/c/sentinel/config.toml".into(),
            storage_available: true,
            recovered_from_unclean_shutdown: false,
        }
    }

    #[test]
    fn every_request_round_trips() {
        for (id, request) in all_requests().into_iter().enumerate() {
            let envelope = RequestEnvelope::new(id as u64, request);
            let frame = encode(&envelope);
            assert_eq!(frame.last(), Some(&b'\n'));
            assert_eq!(frame.iter().filter(|b| **b == b'\n').count(), 1);
            let decoded = decode_request(&frame[..frame.len() - 1]).unwrap();
            assert_eq!(decoded, envelope);
        }
    }

    fn report() -> sentinel_telemetry::model::MonitoringReport {
        use sentinel_telemetry::model::*;
        MonitoringReport {
            phase: MonitoringPhase::Off,
            since: 0,
            sampling: SamplingInfo {
                selected: SamplingProfile::Balanced,
                effective: SamplingProfile::Balanced,
                power_aware: false,
                reason: None,
                intervals_ms: vec![(CollectorKind::Cpu, 2000)],
            },
            collectors: vec![],
            last_sample_at: None,
            error: None,
            enabled_preference: false,
            history_available: true,
            notifications: NotificationStatus {
                enabled: true,
                last_delivery_ok: None,
                last_error: None,
                delivered: 0,
                failed: 0,
            },
        }
    }

    #[test]
    fn every_response_round_trips() {
        let responses = vec![
            Ok(Response::DaemonStatus(sample_status())),
            Ok(Response::Monitoring(sample_status().monitoring)),
            Ok(Response::ApplicationVersion {
                app_version: "0.1.0".into(),
                protocol_version: PROTOCOL_VERSION,
            }),
            Ok(Response::Configuration {
                config: SentinelConfig::default(),
                path: "/c".into(),
            }),
            Ok(Response::ShutdownAccepted),
            Ok(Response::DiscoveryStatus(
                sentinel_inventory::DiscoveryStatus {
                    hardware: sentinel_inventory::JobProgress::idle(
                        sentinel_inventory::DiscoveryJob::Hardware,
                    ),
                    utilities: sentinel_inventory::JobProgress::idle(
                        sentinel_inventory::DiscoveryJob::Utilities,
                    ),
                },
            )),
            Ok(Response::HardwareInventory(
                crate::protocol::InventoryAnswer {
                    inventory: None,
                    origin: None,
                    progress: sentinel_inventory::JobProgress::idle(
                        sentinel_inventory::DiscoveryJob::Hardware,
                    ),
                },
            )),
            Ok(Response::Alerts { alerts: vec![] }),
            Ok(Response::History {
                series: vec![sentinel_telemetry::model::Series {
                    key: sentinel_telemetry::model::SeriesKey::new("cpu.total").unwrap(),
                    unit: "%".into(),
                    source: "/proc/stat".into(),
                    quality: Some(sentinel_telemetry::model::Quality::Calculated),
                    resolution: sentinel_telemetry::model::HistoryResolution::Raw,
                    points: vec![sentinel_telemetry::model::SeriesPoint {
                        t: 1,
                        v: None,
                        min: None,
                        max: None,
                    }],
                }],
            }),
            Ok(Response::Processes(Box::new(
                crate::protocol::ProcessAnswer {
                    monitoring: report(),
                    processes: None,
                },
            ))),
            Ok(Response::Metrics(Box::new(
                crate::protocol::MetricsAnswer {
                    monitoring: report(),
                    snapshot: Some(sentinel_telemetry::model::TelemetrySnapshot::empty()),
                },
            ))),
            Ok(Response::MonitoringReport(Box::new(report()))),
            Ok(Response::Sampling(report().sampling)),
            Err(IpcError::new(ErrorCode::MonitoringUnavailable, "no engine")),
        ];
        for result in responses {
            let envelope = ResponseEnvelope::new(Some(7), result);
            let frame = encode(&envelope);
            assert_eq!(
                decode_response(&frame[..frame.len() - 1]).unwrap(),
                envelope
            );
        }
    }

    #[test]
    fn wire_format_is_stable() {
        let frame = encode(&RequestEnvelope::new(3, Request::GetDaemonStatus));
        assert_eq!(
            String::from_utf8(frame).unwrap(),
            "{\"version\":1,\"id\":3,\"request\":{\"type\":\"GetDaemonStatus\"}}\n"
        );
        let frame = encode(&ResponseEnvelope::new(
            None,
            Err(IpcError::new(ErrorCode::MalformedRequest, "bad")),
        ));
        assert_eq!(
            String::from_utf8(frame).unwrap(),
            "{\"version\":1,\"id\":null,\"outcome\":{\"status\":\"error\",\"error\":\
             {\"code\":\"malformed_request\",\"message\":\"bad\"}}}\n"
        );
    }

    fn code_of(frame: &str) -> (Option<u64>, ErrorCode) {
        let failure = decode_request(frame.as_bytes()).unwrap_err();
        (failure.id, failure.error.code)
    }

    #[test]
    fn malformed_frames_are_rejected() {
        for frame in ["", "not json", "[1,2,3]", "42", "{\"id\":1}", "\u{0}\u{1}"] {
            assert_eq!(
                code_of(frame).1,
                ErrorCode::MalformedRequest,
                "frame {frame:?}"
            );
        }
        // Version present but id missing or not a number.
        assert_eq!(
            code_of("{\"version\":1,\"request\":{\"type\":\"Shutdown\"}}"),
            (None, ErrorCode::MalformedRequest)
        );
        assert_eq!(
            code_of("{\"version\":1,\"id\":\"x\",\"request\":{\"type\":\"Shutdown\"}}"),
            (None, ErrorCode::MalformedRequest)
        );
    }

    #[test]
    fn unsupported_version_is_reported_with_the_request_id() {
        assert_eq!(
            code_of("{\"version\":2,\"id\":9,\"request\":{\"type\":\"GetDaemonStatus\"}}"),
            (Some(9), ErrorCode::UnsupportedVersion)
        );
    }

    #[test]
    fn unknown_or_invalid_requests_are_rejected() {
        for frame in [
            // No such request: in particular nothing resembling command execution.
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"Execute\",\"command\":\"id\"}}",
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"KillProcess\",\"pid\":1}}",
            // Discovery takes no paths and no free-form job names.
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"CancelDiscovery\",\"job\":\"/etc\"}}",
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"GetHardwareInventory\",\"path\":\"/\"}}",
            // Telemetry requests: bounded and typed.
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"GetAlertHistory\",\"limit\":0}}",
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"GetAlertHistory\",\"limit\":1000000}}",
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"GetTelemetryHistory\",\"query\":{\"series\":[\"rm -rf /\"],\"window\":\"hour1\"}}}",
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"GetTelemetryHistory\",\"query\":{\"series\":[],\"window\":\"hour1\"}}}",
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"UpdateSamplingProfile\",\"profile\":\"turbo\"}}",
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"KillProcess\",\"pid\":1234}}",
            "{\"version\":1,\"id\":1,\"request\":\"GetDaemonStatus\"}",
            "{\"version\":1,\"id\":1}",
            // Unknown setting inside a patch.
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"UpdateConfiguration\",\
             \"patch\":{\"cpu_governor\":\"performance\"}}}",
            // Wrong type for a known setting.
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"UpdateConfiguration\",\
             \"patch\":{\"power_mode\":\"ludicrous\"}}}",
            // Empty patch.
            "{\"version\":1,\"id\":1,\"request\":{\"type\":\"UpdateConfiguration\",\"patch\":{}}}",
        ] {
            assert_eq!(
                code_of(frame),
                (Some(1), ErrorCode::InvalidRequest),
                "frame {frame}"
            );
        }
    }

    #[test]
    fn oversized_frames_are_rejected_before_parsing() {
        let frame = vec![b' '; MAX_FRAME_BYTES + 1];
        assert_eq!(
            decode_request(&frame).unwrap_err().error.code,
            ErrorCode::FrameTooLarge
        );
    }

    #[test]
    fn response_with_wrong_version_is_rejected() {
        let mut envelope = ResponseEnvelope::new(Some(1), Ok(Response::ShutdownAccepted));
        envelope.version = 99;
        let frame = encode(&envelope);
        assert!(decode_response(&frame[..frame.len() - 1]).is_err());
        assert!(decode_response(b"garbage").is_err());
        assert!(matches!(
            ResponseEnvelope::new(Some(1), Ok(Response::ShutdownAccepted)).outcome,
            Outcome::Ok { .. }
        ));
    }
}
