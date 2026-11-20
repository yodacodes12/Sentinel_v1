# Sentinel architecture

This document describes what is implemented (Phases 1 to 3) and where later phases plug in. It does
not describe future features as if they existed.

## Processes

Sentinel is three programs that share one backend:

| Binary | Crate | Role |
|---|---|---|
| `sentineld` | `crates/sentinel-daemon` | Background daemon. Owns all backend state. |
| `sentinel` | `apps/cli` | Command-line interface. |
| `sentinel-desktop` | `apps/desktop/src-tauri` | Desktop window. Displays what the daemon reports. |

The daemon and the desktop window have independent lifecycles. Closing the window does not stop
the daemon; stopping the daemon does not close the window (it shows the service as stopped).

## Crates

```text
sentinel-core      errors, XDG paths, instance lock, platform guards, state machines
sentinel-config    versioned TOML configuration: parse, validate, migrate, atomic save
sentinel-logging   JSON-lines file logging (tracing), private files, start-up rotation
sentinel-storage   SQLite: schema migrations and the daemon session history
sentinel-inventory typed inventory model: Field<T> provenance, hardware and utility records,
                   board identity contract, discovery progress, fingerprint, cache format
sentinel-hardware  read-only hardware discovery (SysRoot reader, parsers, 12 modules, manager)
sentinel-utilities read-only utility and service discovery (desktop entries, package
                   databases, PATH, fixed systemd listings)
sentinel-telemetry read-only collectors, central scheduler, counter maths, live series
                   buffers and 30 s bucketing, the telemetry and alert data model
sentinel-alerts    alert rules, the alert state machine, health indicators, desktop
                   notifications over the session D-Bus
sentinel-history   SQLite telemetry and alert history with retention (history.db)
sentinel-ipc       protocol types, frame codec, blocking client
sentinel-daemon    instance lock, socket, async server, request handler, scan coordinator,
                   monitor (the telemetry thread)
sentinel-docs      Phase 6 document discovery: identity normalisation, queries, providers,
                   classification, hardware matching, source trust, the only network module
                   (net.rs), safe downloads
sentinel-client    the single interface used by the CLI and the desktop shell; launcher
```

Dependencies point downwards only. `sentinel-core` depends on no other Sentinel crate.

`sentinel-client` is an addition to the structure proposed in the Phase 1 brief. It exists so the
CLI and the desktop app cannot drift apart: both call `Sentinel::start`, `stop`, `overview`,
`configuration`, `update_configuration` and `monitoring`, and neither touches the socket or the
configuration file directly.

Unit tests live in each crate. The cross-process suites live in the `tests/` package
(`tests/integration`, `tests/safety`), with input files in `tests/fixtures`.

## The daemon

Start-up order in `sentinel_daemon::run`, chosen so that a second daemon exits before it can
disturb the first:

1. Refuse non-Linux systems and refuse to run as root.
2. Create Sentinel's directories with mode `0700`.
3. Take the instance lock. A second daemon stops here with exit code 10.
4. Initialise logging, load configuration, open storage.
5. Bind the socket (removing a stale one), install signal handlers, serve.

**Single instance.** The daemon holds an exclusive `flock` on `daemon.lock` for its whole life.
The kernel releases it when the process ends for any reason, so there is no PID file to go stale.
Clients use the same lock to tell "stopped" from "still shutting down".

**Stale sockets.** A socket file can only be stale if no process holds the lock. The daemon
removes it only after taking the lock, and only if it really is a socket. A regular file or a
symlink at that path is reported and left alone.

**Concurrency.** One thread on a Tokio current-thread runtime. With no clients connected and
monitoring off, the process is parked in `epoll_wait`: no timers, no polling. Monitoring adds
exactly one thread (below) for as long as it is on. Each connection is a task; shared state
is behind a mutex that is never held across an `await`.

**Shutdown.** `SIGTERM`, `SIGINT` or an IPC `Shutdown` request stop the accept loop, give open
connections two seconds to finish, remove the socket, record the clean shutdown and release the
lock last. `SIGHUP` is ignored so closing the launching terminal does not stop the daemon.

**Unexpected termination.** The session table in SQLite records each start and each clean stop.
A session with no recorded stop means the previous daemon died; the next start closes it as
`unclean` and reports `recovered_from_unclean_shutdown` in its status.

**Discovery.** Scans are requested by clients and run by the coordinator
(`discovery.rs`) on one worker thread that exists only while there is work, so an idle daemon is
still single-threaded and never wakes up. One scan runs at a time; duplicates are not queued;
cancellation and a 60 second limit keep the previous result. Completed scans are cached in SQLite
and loaded at start-up as "cached". See [HARDWARE_DISCOVERY.md](HARDWARE_DISCOVERY.md),
[SYSTEM_INVENTORY.md](SYSTEM_INVENTORY.md) and [UTILITY_DISCOVERY.md](UTILITY_DISCOVERY.md).
`SENTINEL_DISCOVERY_ROOT` points discovery at a fixture tree (tests); such inventories carry
`root_override` and are labelled as not describing this machine.

**Monitoring.** `monitoring.rs` owns the telemetry engine on one thread that exists only while
monitoring is on or paused. The thread sleeps on a condition variable until the scheduler's next
due collector, or until the IPC handler signals a command or a settings change, so there is no
fixed tick. Each wake-up runs the due collectors, feeds the live buffers, evaluates alerts,
delivers notifications and publishes the snapshot under a mutex that the IPC handler only ever
holds briefly. Paused: the thread waits without a timeout. Stopped: the thread flushes its
history buffer and exits. See [TELEMETRY_ENGINE.md](TELEMETRY_ENGINE.md).

**Degraded mode.** If the database cannot be opened the daemon logs the error, runs without
session history and reports `storage_available: false`. The unreadable file is never deleted.

## IPC protocol

Transport: Unix domain stream socket at `$XDG_RUNTIME_DIR/sentinel/daemon.sock`. Framing: one
JSON document per line; requests at most 64 KiB, responses at most 16 MiB (an inventory is about
190 KB, the utility catalogue about 170 KB). Unknown fields in a request are rejected.

```json
{"version":1,"id":3,"request":{"type":"GetDaemonStatus"}}
{"version":1,"id":3,"outcome":{"status":"ok","response":{"type":"DaemonStatus", "...": "..."}}}
{"version":1,"id":3,"outcome":{"status":"error","error":{"code":"...","message":"..."}}}
```

Requests (the complete set): `GetDaemonStatus`, `GetApplicationVersion`, `GetConfiguration`,
`UpdateConfiguration { patch }`, `StartMonitoring`, `StopMonitoring`, `PauseMonitoring`,
`ResumeMonitoring`, `Shutdown`, and (Phase 2) `RunHardwareDiscovery` (scan unless a result exists),
`RefreshHardwareInventory` (always scan), `GetHardwareInventory`, `GetDiscoveryStatus`,
`DiscoverSystemUtilities`, `RefreshSystemUtilities`, `GetSystemUtilities`,
`GetHardwareCapabilities`, `CancelDiscovery { job }`, and (Phase 3) `GetMonitoringStatus`,
`GetLatestMetrics`, `GetCpuMetrics`, `GetMemoryMetrics`, `GetProcessMetrics`, `GetBatteryMetrics`,
`GetThermalMetrics`, `GetGpuMetrics`, `GetStorageMetrics`, `GetNetworkMetrics`, `GetActiveAlerts`,
`GetAlertHistory { limit }`, `AcknowledgeAlert { alert_id }`, `GetSamplingProfile`,
`UpdateSamplingProfile { profile, power_aware }`, `GetTelemetryHistory { query }`. Scan requests
return the progress at once; clients poll `GetDiscoveryStatus`. No request carries a path, a
command or free text: a history query is a list of at most 32 validated series keys
(`[A-Za-z0-9._:/@-]`, at most 96 characters) and an enumerated time window.

Requests were added without changing existing ones, so the protocol version stays 1. An older
client works with a newer daemon; a newer client talking to an older daemon gets
`invalid_request` for the new requests.

`Shutdown` is not in the brief's minimum list. It was added because `sentinel stop` must work
without sending signals to processes.

Error codes: `malformed_request`, `unsupported_version`, `invalid_request`, `frame_too_large`,
`monitoring_unavailable`, `invalid_state_transition`, `config_error`, `shutting_down`, `internal`.

Server limits: 32 concurrent connections, 30 second idle timeout per connection, peer user
checked with `SO_PEERCRED`. A malformed line gets an error answer and the connection stays
usable; an oversized frame gets an error and the connection is closed. The client applies a
3 second timeout per request.

### Monitoring commands

| Request | Answer |
|---|---|
| `StartMonitoring` | ok, state `running` once the first round of samples is published; also stores `monitoring_enabled = true` |
| `PauseMonitoring` | ok, state `paused`; error `invalid_state_transition` unless running |
| `ResumeMonitoring` | ok, state `running`; error `invalid_state_transition` unless paused |
| `StopMonitoring` | ok, state `stopped` after the monitor thread has exited; stores `monitoring_enabled = false` |

A request is acknowledged only when the state it asked for holds afterwards. The daemon starts
monitoring at start-up exactly when `monitoring_enabled` is true. Metric requests while
monitoring is off answer with the monitoring report and no snapshot, never with old values.

## Configuration

`config.toml` (version 2) holds `monitoring_enabled`, `power_mode` (the sampling profile:
`eco`, `balanced`, `performance`), `power_aware_sampling`, `notifications_enabled`, `theme`, the
`alerts` thresholds and the history `retention`. Every field takes effect immediately when changed
through the daemon; the monitor thread is told about the change and applies it without a
restart. Version 1 files gain the Phase 3 fields with their defaults.

| Situation | Behaviour |
|---|---|
| File missing | Defaults are used and written |
| File unreadable or invalid | Moved to `config.toml.corrupt-<time>`, defaults written |
| Older version | Migrated and rewritten |
| Newer version | Left untouched; defaults used in memory; updates refused |

While the daemon runs, updates go through it. When it is stopped, `sentinel-client` applies the
same validated update to the file directly. Saves are atomic (temporary file, then rename).

## Desktop application

The frontend (`apps/desktop/src`) is React, TypeScript, Vite and Tailwind CSS:

```text
app/         App root, navigation and hardware catalog, view-model functions
components/  Header, Sidebar, MotherboardDiagram (SVG), icons, primitives
layouts/     AppShell
pages/       Overview, Hardware, Motherboard, Utilities, Services, Alerts, Settings,
             telemetry/ (CPU, Memory, Processes, Battery, Thermal, GPU, Storage, Network tabs)
hooks/       useSentinel (service), useInventory (scans), useTelemetry (metrics, alerts, series),
             useSerialised (polls that never overlap)
components/  also Chart.tsx: SVG line chart that breaks at gaps and says "no data" when off
services/    bridge.ts: the only module that calls Tauri
styles/      design tokens
types/       TypeScript mirrors of the Rust types
```

The Tauri shell exposes twenty-nine commands, each a thin adapter over `sentinel-client`:
`get_overview`, `start_daemon`, `stop_daemon`, `get_configuration`, `update_configuration`,
`monitoring_command`, `get_inventory`, `get_utilities`, `get_discovery_status`, `run_discovery`,
`cancel_discovery`, `get_capabilities`, `launch_utility`, `get_monitoring_status`, `get_metrics`,
`get_processes`, `get_active_alerts`, `get_alert_history`, `acknowledge_alert`, `get_sampling`,
`set_sampling`, `get_telemetry_history`, and since Phase 6 `get_documents`,
`start_document_discovery`, `cancel_document_discovery`, `download_document`,
`remove_document_file`, `clear_document_cache`, `open_document` (documents by numeric ID only). The frontend refreshes service state every five seconds
while the window is visible; while monitoring is active it polls metrics at the CPU sampling
interval (at least 1 s) and the visible charts' history at the same pace; it loads the inventory
when the service starts or stops, polls scan progress only while a scan runs, and does nothing
while the page is hidden (on GNOME Wayland a minimised window still counts as visible, see
[POWER_EFFICIENCY.md](POWER_EFFICIENCY.md)).

Since Phase 4 every background poll goes through `useSerialised` (`services/serialise.ts`): a
poll never starts while the previous one of its kind is still waiting, so a slow daemon delays
updates instead of stacking requests. What a poll result does to the display is decided in
`app/pollState.ts`: a daemon that is not running drops live data (the header says "OFF"); a
daemon that does not answer keeps the last data but marks it "not live", shows "NO REPLY" and
disables the start switch; an error from polling clears itself on the next good answer.

Pages: Overview (identity, monitoring switch, sampling profile, live tiles with sparklines,
active alerts, CPU topology with live per-CPU load), Telemetry (eight tabs with charts and
5 min to 30 day windows), Alerts (active and history, acknowledge), Hardware, Motherboard
(identity, no drawing), Installed Utilities, Services and Startup (read-only), Settings
(monitoring at start-up, notifications, power-aware sampling, every alert threshold, retention).

Since Phase 5 the Hardware page is the **hardware explorer**: an overview, a component tree per
category, logical diagrams (core topology with live load, cache sharing, memory capacity, GPU
functional blocks, disk layout, battery capacity) and an inspector for the selected component.
It is fed by the `ComponentIndex` that `sentinel-client` builds from the inventory (the daemon's
IPC is unchanged) and by the app-wide telemetry snapshot; it has no polling of its own. Details:
[HARDWARE_EXPLORER.md](HARDWARE_EXPLORER.md).

The header has two separate controls: the Sentinel service (the daemon) and monitoring. Every
value on screen carries its state: measured, calculated, estimated, pending, unsupported,
unavailable, permission denied or error. Missing values are never drawn as zero.

## Extension points for later phases

| Future work | Where it plugs in |
|---|---|
| A new collector | A module in `sentinel-telemetry/src/collectors`, a `CollectorKind`, a row in `profile_interval_ms`, and series in `series.rs` |
| A new alert rule | A block in `sentinel-alerts/src/rules.rs` producing an `Observation`; the state machine is shared |
| Predictive analysis (later phase) | Read `history.db` through `sentinel-history`; the series keys are stable |
| Schematic discovery | Consume `BoardIdentity` and `search_terms()` only |
| More hardware | A new module in `sentinel-hardware` (see HARDWARE_DISCOVERY.md) |
| Live device events | Replace manual refresh with udev events feeding the coordinator |
| New stored data | Append a migration to `MIGRATIONS` in `sentinel-storage` |
| New settings | Add a field with a default to `SentinelConfig` and `ConfigPatch`; add a migration step if needed |
| New UI sections | Set `implemented: true` in `app/catalog.ts` and add a page |
| Real motherboard view | Render from verified board documentation once schematic analysis exists |
| Document parsing (Phase 7) | Read files referenced by `DocumentCandidate` (`RetrievalStatus::Downloaded` / `InLibrary`) after `download::verify_stored`; keep `matching` as the gate |
| More document sources | A `Provider` in `sentinel-docs/src/providers`, registered in `Documents::providers`; network only through `net::Client` |
| Physical placement of components | Attach verified positions to `ComponentIndex` IDs (`pci:…`, `usb:…`, `memory/device:…`); the index itself stays logical |

No empty modules were created for these. Each is added when its phase begins.
