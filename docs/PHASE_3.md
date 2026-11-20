# Phase 3 engineering report

Status: **implemented and tested.** Real-time telemetry, monitoring control from the CLI and
the desktop app, alerts with hysteresis, desktop notifications, local history with retention,
and the Eco/Balanced/Performance profiles all work. They are covered by unit, fixture,
integration and safety tests, which all pass, and were checked on the development laptop. The
desktop shell now compiles and runs (the WebKitGTK packages were installed for this phase).

Throughout this report, **measured** means read from the development machine, and **synthetic**
means produced by a test fixture. They are kept apart.

Development machine: Dell Inspiron 15 3535, AMD Ryzen 3 7320U (4 cores, 8 threads), 6.5 GiB RAM,
AMD Radeon 610M (integrated, amdgpu), KIOXIA NVMe SSD, Dell firmware with `dell_smm`,
Ubuntu 26.04.1, kernel 7.0.0, GNOME on Wayland.

## 1. Implemented telemetry collectors

| Collector | Reads | Notes |
|---|---|---|
| CPU | `/proc/stat`, `cpufreq/scaling_cur_freq` | total, per logical CPU, user/system/iowait/irq/softirq/steal, frequency |
| Memory | `/proc/meminfo`, `/proc/vmstat`, `/proc/pressure/memory` | used = total − available, cached, swap, swap activity, PSI |
| System load | `/proc/loadavg`, `/proc/uptime`, `/proc/pressure/{cpu,io}` | |
| Processes | `/proc/PID/{stat,status}`, first 256 bytes of `cmdline` | CPU %, RSS, memory %, threads, owner; program name only |
| Battery and power | `/sys/class/power_supply` | energy- and charge-reporting batteries, AC state, health, runtime estimate |
| Temperatures | hwmon `temp*_input`, thermal zones | evidence-based component association, driver limits |
| Fans | hwmon `fan*_input` | read only; `pwm*` never opened |
| GPU | PCI sysfs of display-class devices, hwmon | amdgpu full; Intel frequency; NVIDIA/nouveau honestly limited |
| Storage | `/proc/diskstats`, `statvfs` on local mounts | physical disks only; busy % labelled as activity |
| Network | `/sys/class/net/*/statistics` | counters only |

Supporting code: a central scheduler (`sentinel-telemetry/src/scheduler.rs`), the engine,
counter maths, live buffers and 30-second bucketing, the alert crate (`sentinel-alerts`: rules,
state machine, health indicators, D-Bus notifications), the history crate (`sentinel-history`),
and the daemon's monitor thread (`sentinel-daemon/src/monitoring.rs`). There are 16 new IPC
requests, CLI `monitor`/`metrics`/`alerts` commands, and desktop Telemetry, Alerts and Settings
pages plus a live Overview. Design: [TELEMETRY_ENGINE.md](TELEMETRY_ENGINE.md).

## 2. Hardware metrics collected on the development machine (measured)

All ten collectors ran on this laptop. Nine reported `ok`; temperatures reported `partial` because
one sensor exists but returns no data.

- CPU: total and all 8 logical CPUs, frequency per CPU (`amd-pstate-epp`), load, CPU and I/O PSI.
- Memory: total 6.5 GiB, available, cached, swap (a 9 GiB swap file), swap activity, PSI.
- Processes: about 330, all readable as this user (0 inaccessible).
- Battery (BAT1, charge-reporting): level, charge now/full/design (2.99 / 3.65 Ah), voltage,
  current, power (calculated as current × voltage), health 82 % (estimated), AC adapter state.
- Temperatures: 8 readable sensors: `k10temp` Tctl (CPU, high confidence), `nvme` Composite and
  Sensor 1 (storage, high), `amdgpu` edge (GPU, high), `dell_smm` CPU/SODIMM/Ambient (low
  confidence: firmware labels), ACPI thermal zone (unknown location). Driver limits are known
  for 2 of them (NVMe critical 84.9 °C, ACPI zone critical 125 °C).
- Fan: one, `dell_smm` "Processor Fan" (500 RPM at idle).
- GPU: Radeon 610M at `0000:03:00.0`: busy %, shared memory used, temperature, power, clock.
- Storage: `nvme0n1` read/write throughput, operations, busy; `/` and `/boot/efi` usage.
- Network: `wlp2s0` and `docker0` throughput, packets, errors, drops.
- Health indicators for all nine areas.

## 3. Metrics unavailable on the development machine (measured)

| Metric | Why |
|---|---|
| Battery temperature | `BAT1/temp` exists but returns "No such device"; shown as not provided. The `BAT1` hwmon `temp1` is the same value and is the one "partial" sensor |
| Battery energy (Wh) | The battery reports charge (Ah), not energy; power is calculated |
| Battery cycle count | Reported as 0, which means "not counted" (`unavailable`) |
| Battery technology | Reported as "Unknown" by the firmware |
| Fan min/max | Not exposed by `dell_smm` |
| Limits for 7 of 9 temperature sensors | Not exposed by their drivers; no alert without a user threshold |
| Intel/NVIDIA GPU metrics | Hardware not present; covered by fixtures only |

## 4. CPU and memory calculation validation

Synthetic (exact): the fixture tests feed known `/proc/stat` counters through the real engine
and assert exact results, e.g. per-CPU 75 % and 0 %, total 30 %, user 20 %, iowait 20 %; a
counter reset and a hot-plugged CPU yield no value. Memory: 8 000 000 kB total, 2 000 000 kB
available → 75.0 % used. Process CPU: 150 ticks in 1.5 s at 100 Hz → 100 %. RSS: 102 400 pages ×
4 KiB = 10 % of 4 000 MiB. The integration suite repeats this through the daemon with a writer
thread advancing every counter at a fixed 3:1 busy:idle ratio, so any delta must read exactly
75.0 %; it does.

Measured (cross-checks against independent tools on this laptop):

| Check | Sentinel | Reference |
|---|---|---|
| Memory used | 4 342 890 496 B | `free -b`: 4 341 747 712 B (read a moment apart; 0.03 % apart) |
| Memory available | 2 597 117 952 B | `free -b`: 2 598 260 736 B |
| Process RSS, 6 largest | e.g. 387 456 kB, 349 188 kB | `ps -o rss`: 388 616 kB, 349 792 kB (each within 0.5 %, read a moment later) |
| Threads per process | 14, 22, 30, 41, 17, 43 | `ps -o nlwp`: identical |
| Process count | 332 | `/proc` entries: 334 a moment later |
| CPU total over 5 s | mean of five 1 s samples: 9.0 % | independent `/proc/stat` delta over the same 5 s: 8.1 % (different windows; the first sample included starting the CLI) |

## 5. Battery estimation methodology and accuracy limitations

Runtime = remaining energy (or charge) / mean discharge power (or current) over the last
5 minutes. It is shown only while discharging, after at least 4 valid samples spanning at least
60 s, and only when the coefficient of variation of the power is below 0.8. Confidence is high
below 0.15 with 8+ samples, medium below 0.35, otherwise low. It is rounded to whole minutes,
labelled as an estimate, and cleared at once on AC. Health = full / design capacity, labelled
estimated.

Synthetic: a steady 10 W discharge gives an estimate within 2 minutes of energy / power; unstable
readings and missing power are refused (unit and fixture tests). Measured: on this laptop the
estimator was observed in its "calculating" state on battery and "not discharging" on AC. A full
discharge comparison was not run (it needs several controlled hours), so **the estimate's accuracy
on real hardware is not established**. See [MONITORING_LIMITATIONS.md](MONITORING_LIMITATIONS.md).

## 6. GPU telemetry support

amdgpu: utilisation, VRAM/GTT, temperature, power, clock. **Measured** on the Radeon 610M.
Intel i915/xe: GT frequency only, since utilisation needs perf counters. NVIDIA proprietary: listed,
with the reason it has no utilisation; `nvidia-smi` is never run. nouveau: hwmon temperature.
Intel, NVIDIA and nouveau behaviour is **synthetic only**. Works with no GPU at all
(`no_sensors_means_unsupported_not_zero`).

## 7. Thermal and fan sensor support

hwmon and thermal zones are discovered at every sample, so devices that appear later are picked
up. Values outside −60…250 °C are errors. The kernel's hwmon mirror of a thermal zone is skipped
(it had been counted twice; fixed in this phase). Component association follows evidence (driver →
high, wireless driver → medium, firmware label → low, otherwise unknown). Fans come from hwmon
only. Fixture tests cover parsing, malformed values, `-1` RPM and the untouched `pwm1` file.

Measured cost: on this laptop a `dell_smm` read takes about 1.2 ms (firmware SMM call) and an NVMe
temperature read about 3 ms, which makes the temperature collector the slowest regular one
(14–17 ms per run).

## 8. Alert rules and threshold behaviour

13 rules: `cpu_high`, `load_high`, `memory_low`, `memory_pressure`, `swap_activity`,
`temperature_high`, `battery_low`, `battery_health`, `storage_low`, `disk_busy`, and three that are
off until the user sets a limit (`battery_temperature`, `battery_discharge_high`, `network_high`).
Each has a sustained duration, a hysteresis band and a clear time. Alerts are deduplicated, and a
cooldown (600 s) stops repeat notifications. Every threshold is editable in Settings and
`config.toml`. Temperature alerts use only driver limits or the user's own threshold. Table and
state machine: [ALERT_ENGINE.md](ALERT_ENGINE.md).

Synthetic, through the real daemon: 3 % available memory raises exactly one critical
`memory_low` alert with evidence "3.0% of memory available."; at 12 % (inside the band) it stays
active; at 57 % it resolves only after 30 s clear. The acknowledgement sticks and is stored.
Measured: no alert fired on the development machine during the session; every health indicator
read "normal".

## 9. Notification integration

A minimal D-Bus client calls only `org.freedesktop.Notifications.Notify` on the user's session bus
(no actions, markup escaped). The user's preference is respected. Failures are recorded and
monitoring continues. Synthetic: through a fake session bus in the sandbox, one alert gives exactly
one notification titled "Sentinel — Low available memory", the recovery gives none, disabling
notifications sends none, and with no bus a failure is recorded while the alert stays active.
Measured: `examples/bus_probe.rs` against this GNOME session's real bus was accepted and routed
(answer `ServiceUnknown` for the deliberately unowned name, so nothing was displayed). **A
notification was not displayed on the real desktop in this phase**; the message format the real bus
accepted is the same one the fake bus received.

## 10. Sampling profiles

| | Eco | Balanced | Performance |
|---|---|---|---|
| CPU, memory, storage, network, load | 5 s | 2 s | 1 s |
| Processes, temperatures, fans, GPU | 10 s | 5 s | 2 s |
| Battery | 30 s | 15 s | 10 s |

Plus: processes once a minute unless someone is looking at them, a 5-minute re-check for
unsupported sources, exponential backoff for failures, and optional power-aware Eco on battery
(integration-tested: unplugging switches to Eco with a stated reason; turning it off restores the
choice). Profile changes apply to the running monitor at once. Measured costs per profile are
in §12.

## 11. Database and retention implementation

`history.db` (SQLite, `0600`): `series` (key, unit, **source**, **quality**), `samples` (30 s
average/min/max/count), `hourly` (sample-weighted roll-ups), `alerts`, `meta`. Schema migrations
are versioned; migration 2, added in this phase, adds source and quality, and a test upgrades a
version-1 database. Writes are batched: one transaction every 5 minutes (or 4 000 rows), and on
pause and stop. Retention defaults: 24 h detail, 30 days hourly, 90 days alerts, configurable,
enforced at start and hourly (unit-tested). Process data is never stored. Measured: 470 rows per
5-minute flush on this laptop, about 200–360 KiB written per 5 minutes including the journal and
log. Integration-tested: history survives a daemon restart, and returns the stored values with
their provenance.

## 12. Resource benchmarks for every monitoring mode (measured)

Release build, real `/proc` and `/sys`, nothing talking to the daemon during each window. Full
method and per-collector times: [POWER_EFFICIENCY.md](POWER_EFFICIENCY.md).

| Mode | CPU (% of one core) | Wake-ups/s | RSS | Threads |
|---|---|---|---|---|
| Daemon running, monitoring off | 0.000 | 0.0 | 4.4 MiB | 1 |
| Monitoring on, Eco | 0.21 | 1.05 | 7.0 MiB | 2 |
| Monitoring on, Balanced | 0.48 | 2.38 | 7.3 MiB | 2 |
| Monitoring on, Performance | 0.94 | 5.18 | 8.1 MiB | 2 |
| Desktop window open (Balanced): daemon | 0.54 | 4.87 | 7.8 MiB | 2 |
| Desktop window open: the desktop app itself | 1.81 | 98 | 422 MiB | 99 (3 processes) |
| Desktop window closed, monitoring on (Balanced) | 0.47 | 2.24 | 8.5 MiB | 2 |

Battery impact was **not measured**. The charger was connected during the run, and Sentinel's
share is far below the noise of the system's own power draw. No battery-life claim is made. The
desktop app (WebKit) costs several times more than the daemon itself.

A first benchmark run had to be discarded because of two mistakes in my script and build. It
counted wake-ups only on the daemon's main thread. And the desktop binary had been built with
plain `cargo build`, which without Tauri's `custom-protocol` feature points the window at the dev
server, so it never talked to the daemon. The numbers above come from the corrected runs; the
README now warns about the build trap.

## 13. Unit and integration test results (final run)

| Suite | Tests | Result |
|---|---|---|
| Rust, whole workspace (`cargo test --workspace`) | 342 | all pass |
| of which new or rewritten in Phase 3: telemetry unit 31, telemetry fixtures 13, alerts 9, history 6, daemon monitor 3, integration (telemetry) 9, safety 1 rewritten + 4 updated | | |
| Frontend (`npm test`) | 57 | all pass |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings` (desktop shell included) | | clean |
| `tsc --noEmit`, `vite build` | | clean |

The cross-process suites (integration, safety, hardware) passed twice more in a row afterwards.
The new telemetry integration tests take about 50 s, mostly because the alert test deliberately
waits out the 30 s clear time.

Plan §24 coverage: every listed unit calculation has a test (CPU, per-core, counter deltas,
memory, process CPU and memory, battery health and runtime, temperature conversion, fan parsing,
GPU parsing, storage and network throughput, thresholds, hysteresis, cooldown, recovery, profile
selection, retention). Integration tests cover start/stop, pause/resume, CLI controls, the desktop
client interface (`desktop_interface_talks_to_the_daemon` uses the same `sentinel-client` calls as
the Tauri commands), daemon restart, collector failure and recovery, database writes, history
retrieval, alert generation, notification delivery, power-mode changes and all collectors running
together. No stress test was run on the real machine.

Desktop UI: the production bundle was rendered in headless Firefox with every desktop command
answered from the real daemon's data on this laptop (Overview, Telemetry, Alerts, Settings;
screenshots in `target/ui-phase3-*.png`). The real Tauri window was started, and while open it
answered the daemon at the expected pace (66 daemon wake-ups in 20 s). Its controls were **not
clicked by hand in this phase**.

## 14. Safety verification

The safety suite (20 tests) passes, with these Phase 3 changes:

- `monitoring_state_is_reported_accurately` replaces the Phase 1 "never active" test: off by
  default, start/pause/resume/stop are reported exactly, the preference alone starts nothing, and
  a restart follows the preference.
- The read-only checks now cover `sentinel-telemetry`: no write API, all file access through
  `SysRoot` (which refuses `environ`, process memory and identifier files).
- The IPC pin lists the 16 new requests and allows only typed payloads (`u32`, `u64`, a profile
  enum, `Option<bool>`, a history query of validated keys and an enumerated window).
- The desktop command list is pinned at 22; the webview's permissions are unchanged (four window
  controls).
- `only_sentinels_own_files_are_written` allows `history.db` and nothing else new.

Plan §24 safety items: no process terminated (bystander test), no service, CPU/GPU, power,
battery or network setting changed (no such code exists; source scans; `pwm1` untouched in a
fixture), no packages, no repair commands, no root, no shell, monitoring off stops collection (the
monitor thread exits and the daemon performs zero reads in 3 s; measured idle at 0 CPU and
0 wake-ups), no fabricated values (`unsupported`, never 0; frontend honesty tests), no environment
or arguments collected (secrets planted in a fixture's `cmdline` and `environ` never appear in any
output).

## 15. Known limitations

See [MONITORING_LIMITATIONS.md](MONITORING_LIMITATIONS.md). In short: one laptop tested;
Intel and NVIDIA GPU paths only on fixtures; battery runtime accuracy not validated against a real
discharge; no battery temperature on this machine; most sensors have no driver limit, so no
temperature alert without a user threshold; RSS double-counts shared memory; `scaling_cur_freq` is
an estimate; short-lived processes are missed; NVMe busy % is not saturation; notifications need a
freedesktop service; suspend/resume not tested on real cycles.

## 16. Outstanding issues

1. **Desktop app overhead.** The WebKit window uses about 1.8 % of one core, roughly 98 wake-ups/s
   and 422 MiB, several times the daemon. Worth profiling (re-render frequency of the Overview's
   charts and topology) in a later phase.
2. **Real-window interaction not exercised by hand.** Click through the monitoring switch, the
   profile selector, an acknowledgement and the Settings page in the real window to confirm.
3. **A notification on the real desktop** has not been displayed. To check, set
   `memory_available_low_percent` above the current free memory with notifications on.
4. **Battery runtime accuracy** needs a controlled discharge test.
5. **The temperature collector's cost on Dell machines** (`dell_smm`, 1.2 ms per read) could
   justify reading `dell_smm` less often than other sensors.
6. **Fixed in this phase, noted for the record:** an alert acknowledgement could be dropped when
   no collector was due (it is now applied and stored immediately); thermal zones were counted
   twice; history lacked source and quality; and several UI and code texts still described
   Phase 1 ("no monitoring engine", "per-core utilisation is not monitored").

## 17. Commands for starting, stopping and inspecting monitoring

```bash
sentinel start                               # start the daemon
sentinel monitor start                       # turn monitoring on (remembered across restarts)
sentinel monitor status [--json]             # state, profile, each collector's health
sentinel metrics                             # all live measurements
sentinel metrics cpu|memory|processes|battery|thermal|gpu|storage|network [--json]
sentinel monitor profile eco|balanced|performance [--power-aware on|off]
sentinel monitor pause | resume
sentinel alerts [--history] [--json]
sentinel alerts ack <id>
sentinel monitor stop                        # turn monitoring off (history is kept)
sentinel ui                                  # the desktop app: switch in the header and Overview
```

`sentinel metrics` exits with code 13 when monitoring is off. Build the desktop app with
`npm run desktop:build` (see README).

Phase 3 stops here. Phase 4 will not be started without your instruction.
