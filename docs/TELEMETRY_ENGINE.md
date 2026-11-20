# Telemetry engine

How Sentinel measures the computer while monitoring is on. Metric definitions are in
[METRIC_REFERENCE.md](METRIC_REFERENCE.md), alerts in [ALERT_ENGINE.md](ALERT_ENGINE.md), measured
overhead in [POWER_EFFICIENCY.md](POWER_EFFICIENCY.md), and what cannot be measured in
[MONITORING_LIMITATIONS.md](MONITORING_LIMITATIONS.md).

## Rules the engine follows

- **Off by default.** Nothing is sampled until the user turns monitoring on. Off means the monitor
  thread does not exist and the daemon reads nothing (safety test
  `monitoring_off_stops_collection_and_the_daemon_goes_quiet`).
- **Read only.** Collectors read `/proc` and `/sys` through `SysRoot`, the same reader discovery
  uses. They never open a control file (`pwm*`, `scaling_governor`, charge thresholds...), and
  the crate has no write API.
- **No invented numbers.** Every value is a `Reading`: a value (optional), a state and a quality.
  A missing source is `unsupported`, not `0`; a rate's first sample is `pending`; a counter that
  went backwards is `error` for that interval.
- **One failure stays local.** Each collector runs on its own; a failure becomes that collector's
  health and its readings' states. The others continue.

## Pieces

```text
crates/sentinel-telemetry
  model.rs       Reading<T>, ReadingState, Quality, every sample type, alerts, history types
  counters.rs    pure parsers and counter maths (/proc/stat, diskstats, pid stat, vmstat, PSI...)
  collectors/    cpu, memory, processes, battery, thermal (+ fans), gpu, storage, network, load
  engine.rs      owns the collectors and their previous counters; runs one; keeps the snapshot
  scheduler.rs   the central schedule: intervals, grid alignment, backoff, process interest
  runtime.rs     battery runtime estimator
  series.rs      snapshot -> series points; live ring buffers; 30 s bucketing for history
crates/sentinel-daemon/src/monitoring.rs   the monitor thread and its IPC-facing handle
crates/sentinel-alerts                     rules, alert state machine, health, notifications
crates/sentinel-history                    history.db
```

## The monitor thread

One thread, started by `StartMonitoring` and ended by `StopMonitoring`. Each pass of its loop:

1. Take any command (pause, resume, stop) or settings change from the IPC side.
2. Ask the scheduler what is due. If nothing is, sleep on a condition variable until the next
   due time or a command, whichever comes first. There is no fixed tick.
   Before sampling, compare how far the wall clock and the monotonic clock moved since the last
   pass. More than 5 s apart means the system was suspended (the monotonic clock stops) or its
   time was set: open buckets are closed, a gap is marked, pending alert conditions are
   forgotten (so a sustained-duration check never counts the sleep), and counters and the
   schedule start afresh, exactly as after a pause.
3. Run each due collector, record its health with the scheduler, turn its section into series
   points, and add them to the 30-second bucketer.
4. If the battery collector ran, re-evaluate power-aware sampling.
5. Evaluate the alert rules on the new snapshot, deliver notifications, record alert changes.
6. Compute the health indicators and publish the snapshot, collector statuses and alerts.
7. Write closed buckets to `history.db` if a flush is due; run retention once an hour.

`StartMonitoring` returns once the first round has been published, so "running" always comes with
data. **Pause** closes open buckets, writes them, marks a gap in the live series and waits for a
command with no timeout. **Resume** starts with fresh counters, so no rate spans the pause.
**Stop** flushes, marks a gap, clears the live snapshot and exits the thread. If the thread does
not exit within 5 s (a collector stuck in a hung sensor read), `StopMonitoring` reports "still
stopping" and the daemon keeps owning the thread; `StartMonitoring` is refused until it has
exited. There is never more than one monitor thread.

## Sampling profiles

The scheduler holds one interval per collector. The profile is the `power_mode` setting.

| Collector | Eco | Balanced (default) | Performance |
|---|---|---|---|
| CPU, memory, storage, network, system load | 5 s | 2 s | 1 s |
| Processes, temperatures, fans, GPU | 10 s | 5 s | 2 s |
| Battery | 30 s | 15 s | 10 s |

Adaptive rules on top of the table:

- **Grid alignment.** A collector's next run is `previous due time + interval`, and collectors due
  within 0.25 s of each other run together. Collectors with related intervals therefore wake
  the thread at the same moments. After a suspend or a slow run the grid restarts from now
  instead of catching up.
- **Processes on demand.** The process collector is the most expensive one (it reads three files
  per process). It runs at its profile interval only while someone has asked for process data in
  the last 60 seconds (the Processes tab, `sentinel metrics processes`); otherwise once a minute.
- **Unsupported sources** (no GPU, no fans, no battery) are checked again every 5 minutes, in
  case a device appears.
- **Failures back off.** A failing collector's interval doubles with each consecutive failure,
  up to 5 minutes, and returns to normal after one good run.
- **Power-aware sampling** (`power_aware_sampling`, off by default): while the battery collector
  reports that the computer runs on battery, the effective profile is Eco, and the sampling info
  says so and why. The user's choice is restored as soon as mains power is reported. An unknown
  power source keeps the user's choice.

Changing the profile, thresholds or notifications takes effect on the running thread
immediately; no restart is needed.

## Live data and history

- **Live buffers** (memory only): up to 900 points per series and 512 series. At 1 s that is
  15 minutes; at 5 s, 75 minutes. The 5 and 15 minute chart windows read from here at full
  resolution. Gaps (pause, stop, restart, a collector failure) are stored as gaps and drawn as
  breaks, never bridged.
- **History** (`$XDG_DATA_HOME/sentinel/history.db`): points are averaged into 30-second buckets
  (average, minimum, maximum, sample count). Buckets are written in one transaction every
  5 minutes, or sooner past 4 000 rows, and always on pause and stop, so the disk is not touched
  per sample. Completed hours are rolled up into hourly rows weighted by sample count.
  Every series row records its unit, its source (e.g. `/proc/stat`) and its quality (direct,
  calculated or estimated); the device or process is part of the series key
  (`disk.nvme0n1.read`, `temp.hwmon4/temp1`). History answers return all three.
- **Windows:** 5 min and 15 min from memory; 1 h and 24 h from the 30-second table; 7 and 30
  days from the hourly table. A query names at most 32 series, and the resolution bounds each
  one: at most 2 880 points (24 h of 30 s buckets) or 900 live points.
- **Retention** (configurable): 24 hours of 30-second detail, 30 days of hourly data, 90 days of
  alerts. Enforced at monitoring start and every hour after.
- **Never stored:** process data and per-CPU frequencies (series `proc.*` and `*.freq` stay in
  memory only).

## Errors and recovery

| Situation | What happens |
|---|---|
| A sensor file is missing or disappears | That reading becomes `unsupported` or `unavailable`; the collector continues |
| A device disappears (USB disk, interface) | It drops out of the next sample; its previous counters are discarded |
| CPU hot-plug | A CPU with no previous counters is `pending` for one interval |
| A process exits while being read | Skipped silently; a reused pid (different start time) is never compared with the old process |
| Permission denied | Counted and reported (`permission_denied`, "N processes not readable"); never bypassed |
| Unsupported GPU telemetry | The GPU is listed with its driver and an explanation (e.g. NVIDIA without sysfs utilisation) |
| Malformed value | `error` for that reading; never parsed into a number |
| Counter reset or wrap | `error` for that interval; the next interval is measured normally |
| History database fails | Logged; monitoring continues with live data only; `history_available: false` |
| Notification service missing | Recorded in the notification status; the alert is still stored |
| IPC client disconnects | Nothing to do: clients only read; the thread does not depend on them |
| Monitor thread cannot start | `StartMonitoring` fails with the reason; the phase is `error` |
| A collector hangs in a read | Stop waits 5 s, then reports "still stopping"; no second monitor can start until the first exits |
| System suspend and resume, or the clock is set | Detected from the wall and monotonic clocks; sampling restarts as after a pause (see above) |
| Core `/proc` file missing (`stat`, `meminfo`, `diskstats`, `loadavg`) | The collector is `failing` (abnormal on Linux; e.g. a restricted container); others continue |

## Testing

- Pure calculations (`counters.rs`, `runtime.rs`, `scheduler.rs`, `series.rs`) have unit tests on
  literal input.
- `crates/sentinel-telemetry/tests/fixture_collectors.rs` runs every collector through the real
  engine against a synthetic `/proc` and `/sys`; each expected number is derived in a comment.
- `tests/integration/telemetry.rs` runs the real daemon against a synthetic laptop
  (`SENTINEL_DISCOVERY_ROOT`) and checks CLI controls, restart, history, failure isolation,
  alerts, notifications (to a fake session bus) and power-aware sampling.
- `tests/integration/reliability.rs` (Phase 4): a frozen daemon, connection churn, a crash during
  monitoring, rapid control changes, invalid settings, and machines without a battery or GPU.
- `monitoring.rs` unit tests: a collector hung in a FIFO read never leads to a second monitor
  thread; suspend and clock changes are detected.
- On the development machine, values were cross-checked against the kernel's own counters
  (Phase 3: `free`, `ps`, `/proc/stat`; Phase 4: 31 metrics, see
  [DESKTOP_INTEGRATION.md](DESKTOP_INTEGRATION.md#measurement-accuracy-on-real-hardware)).
