//! Answers to the Phase 3 monitoring requests.

use sentinel_config::ConfigPatch;
use sentinel_core::MonitoringCommand;
use sentinel_history::{History, SeriesInfo};
use sentinel_ipc::{ErrorCode, IpcError, MetricsAnswer, ProcessAnswer, Request, Response};
use sentinel_telemetry::model::{
    HistoryQuery, HistoryResolution, Series, SeriesKey, SeriesPoint, TelemetrySnapshot, TimeWindow,
};

use crate::context::{DaemonContext, unix_now};
use crate::monitoring::{MonitorSettings, mode_of};

/// Keep only the sections a specific request asks for.
fn filter(request: &Request, s: TelemetrySnapshot) -> TelemetrySnapshot {
    let mut out = TelemetrySnapshot::empty();
    match request {
        Request::GetCpuMetrics => {
            out.cpu = s.cpu;
            out.load = s.load;
        }
        Request::GetMemoryMetrics => {
            out.memory = s.memory;
            out.top_processes = s.top_processes;
        }
        Request::GetBatteryMetrics => out.battery = s.battery,
        Request::GetThermalMetrics => {
            out.thermal = s.thermal;
            out.fans = s.fans;
        }
        Request::GetGpuMetrics => out.gpu = s.gpu,
        Request::GetStorageMetrics => out.storage = s.storage,
        Request::GetNetworkMetrics => out.network = s.network,
        _ => return s,
    }
    out
}

pub fn monitoring_command(
    ctx: &mut DaemonContext,
    command: MonitoringCommand,
) -> Result<Response, IpcError> {
    let status = ctx.monitoring.execute(command)?;
    // Start and stop are the user's choice and survive restarts; pause is
    // temporary.
    let preference = match command {
        MonitoringCommand::Start => Some(true),
        MonitoringCommand::Stop => Some(false),
        _ => None,
    };
    if let Some(enabled) = preference
        && ctx.config.config().monitoring_enabled != enabled
        && let Err(error) = ctx.config.update(&ConfigPatch {
            monitoring_enabled: Some(enabled),
            ..Default::default()
        })
    {
        tracing::warn!(%error, "could not save the monitoring preference");
    }
    Ok(Response::Monitoring(status))
}

pub fn handle(ctx: &mut DaemonContext, request: Request) -> Result<Response, IpcError> {
    let preference = ctx.config.config().monitoring_enabled;
    match request {
        Request::GetMonitoringStatus => Ok(Response::MonitoringReport(Box::new(
            ctx.monitoring.report(preference),
        ))),
        Request::GetProcessMetrics => Ok(Response::Processes(Box::new(ProcessAnswer {
            processes: ctx.monitoring.processes(),
            monitoring: ctx.monitoring.report(preference),
        }))),
        Request::GetLatestMetrics
        | Request::GetCpuMetrics
        | Request::GetMemoryMetrics
        | Request::GetBatteryMetrics
        | Request::GetThermalMetrics
        | Request::GetGpuMetrics
        | Request::GetStorageMetrics
        | Request::GetNetworkMetrics => {
            let snapshot = ctx.monitoring.snapshot().map(|s| filter(&request, s));
            Ok(Response::Metrics(Box::new(MetricsAnswer {
                monitoring: ctx.monitoring.report(preference),
                snapshot,
            })))
        }
        Request::GetActiveAlerts => Ok(Response::Alerts {
            alerts: ctx.monitoring.active_alerts(),
        }),
        Request::GetAlertHistory { limit } => {
            let stored = ctx
                .monitoring
                .history_path()
                .and_then(|p| History::open_read_only(p).ok().flatten())
                .and_then(|h| h.alerts(limit).ok());
            let mut alerts = ctx.monitoring.recent_alerts();
            for a in stored.unwrap_or_default() {
                if !alerts.iter().any(|x| x.id == a.id) {
                    alerts.push(a);
                }
            }
            alerts.sort_by(|a, b| b.detected_at.cmp(&a.detected_at).then(b.id.cmp(&a.id)));
            alerts.truncate(limit as usize);
            Ok(Response::Alerts { alerts })
        }
        Request::AcknowledgeAlert { alert_id } => {
            if let Some(a) = ctx.monitoring.acknowledge(alert_id) {
                return Ok(Response::Alerts { alerts: vec![a] });
            }
            // An alert from an earlier session: only its stored record changes.
            let found = ctx
                .monitoring
                .history_path()
                .and_then(|p| History::open(p).ok())
                .and_then(|mut h| {
                    let mut a = h
                        .alerts(MAX_LOOKUP)
                        .ok()?
                        .into_iter()
                        .find(|a| a.id == alert_id)?;
                    a.acknowledged = true;
                    h.upsert_alert(&a).ok()?;
                    Some(a)
                });
            found
                .map(|a| Response::Alerts { alerts: vec![a] })
                .ok_or_else(|| {
                    IpcError::new(
                        ErrorCode::InvalidRequest,
                        format!("there is no alert {alert_id}"),
                    )
                })
        }
        Request::GetSamplingProfile => Ok(Response::Sampling(ctx.monitoring.sampling())),
        Request::UpdateSamplingProfile {
            profile,
            power_aware,
        } => {
            let patch = ConfigPatch {
                power_mode: Some(mode_of(profile)),
                power_aware_sampling: power_aware,
                ..Default::default()
            };
            ctx.config
                .update(&patch)
                .map_err(|e| IpcError::new(ErrorCode::ConfigError, e.to_string()))?;
            let settings = MonitorSettings::from_config(ctx.config.config());
            ctx.monitoring.apply_settings(settings.clone());
            tracing::info!(?profile, ?power_aware, "sampling profile updated");
            // The monitor applies it on its next wake-up; the answer is what
            // it will apply, not what was in effect a moment ago.
            Ok(Response::Sampling(ctx.monitoring.sampling_for(&settings)))
        }
        Request::GetTelemetryHistory { query } => Ok(Response::History {
            series: history(ctx, &query),
        }),
        _ => Err(IpcError::new(
            ErrorCode::Internal,
            "not a telemetry request",
        )),
    }
}

const MAX_LOOKUP: u32 = 10_000;

fn series_of(
    key: &SeriesKey,
    info: SeriesInfo,
    resolution: HistoryResolution,
    points: Vec<SeriesPoint>,
) -> Series {
    Series {
        key: key.clone(),
        unit: info.unit,
        source: info.source,
        quality: info.quality,
        resolution,
        points,
    }
}

fn history(ctx: &DaemonContext, query: &HistoryQuery) -> Vec<Series> {
    let now_ms = unix_now() * 1000;
    let since = now_ms.saturating_sub(query.window.seconds() * 1000);
    let db = ctx
        .monitoring
        .history_path()
        .and_then(|p| History::open_read_only(p).ok().flatten());
    query
        .series
        .iter()
        .map(|key| {
            let live = ctx.monitoring.live_series(key, since);
            match query.window {
                TimeWindow::Minutes5 | TimeWindow::Minutes15 => match live {
                    Some((meta, points)) => Series {
                        key: key.clone(),
                        unit: meta.unit.to_string(),
                        source: meta.source.to_string(),
                        quality: Some(meta.quality),
                        resolution: HistoryResolution::Raw,
                        points,
                    },
                    None => series_of(
                        key,
                        SeriesInfo::default(),
                        HistoryResolution::Raw,
                        Vec::new(),
                    ),
                },
                TimeWindow::Hour1 | TimeWindow::Hours24 => {
                    let (mut info, mut points) = db
                        .as_ref()
                        .and_then(|h| h.query(key, since, HistoryResolution::Detailed).ok())
                        .unwrap_or_default();
                    // Recent samples not yet written to the database.
                    let last = points.last().map(|p| p.t + 30_000).unwrap_or(since);
                    if let Some((meta, tail)) = live {
                        // The live buffer knows the current provenance.
                        info = SeriesInfo {
                            unit: meta.unit.to_string(),
                            source: meta.source.to_string(),
                            quality: Some(meta.quality),
                        };
                        points.extend(tail.into_iter().filter(|p| p.t >= last));
                    }
                    series_of(key, info, HistoryResolution::Detailed, points)
                }
                TimeWindow::Days7 | TimeWindow::Days30 => {
                    let (info, points) = db
                        .as_ref()
                        .and_then(|h| h.query(key, since, HistoryResolution::Hourly).ok())
                        .unwrap_or_default();
                    series_of(key, info, HistoryResolution::Hourly, points)
                }
            }
        })
        .collect()
}
