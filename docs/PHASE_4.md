# Phase 4: desktop integration and reliability — completion report

Phase 4 added no product features. It checked that the daemon, CLI, IPC, telemetry, alerts,
notifications, history and the desktop window work together on a real Linux desktop, fixed what
did not, and recorded what has and has not been validated on real hardware.

Detailed reports: [DESKTOP_INTEGRATION.md](DESKTOP_INTEGRATION.md) (window, navigation,
accuracy), [IPC_RELIABILITY.md](IPC_RELIABILITY.md) (daemon and IPC faults),
[NOTIFICATION_VALIDATION.md](NOTIFICATION_VALIDATION.md), [HARDWARE_COMPATIBILITY.md](HARDWARE_COMPATIBILITY.md)
(matrix and remaining physical validation), [POWER_EFFICIENCY.md](POWER_EFFICIENCY.md) (resource
measurements and the long run).

## A. Executive summary

- **The audit and testing found 14 product defects and 3 test, documentation or build
  problems**, all fixed and each covered by a new or extended test (register below); for the
  battery alert hold and the three Settings and polling defects the new tests were also run
  against the old code and fail there. The most serious: a hung collector could lead to two monitor
  threads; a daemon that stopped answering was shown as stopped (and the window offered to start
  a second one) or, because one timed-out request discarded the other answer, as still
  connected; background requests piled up against a slow daemon; Settings lost unsaved edits
  every 5 s; restarting the daemon repeated notifications the user had just seen; the battery
  "average draw" was in amperes on this laptop; and a single battery reading could raise a
  low-battery alert.
- **The real desktop window was tested end to end** (Tauri + WebKitGTK on GNOME 50.1, driven
  through the accessibility tree): 12 scenarios including the daemon stopped, restarted, frozen
  and killed while the window is open. All pass on the final build.
- **A real desktop notification** was delivered once through GNOME Shell and was not repeated
  while active, on recovery, inside the cooldown or after a daemon restart.
- **Measurements agree with the kernel**: 30 of 31 comparisons within tolerance (CPU within 0.07
  points per core, I/O and network within 2.5 %, memory exact); the 31st is a sensor the kernel
  itself cannot read, correctly shown as not provided.
- **All 13 alert rules** now have rule-by-rule tests (they had none of their own).
- **Long run**: 2.5 hours (9001 s) of continuous monitoring on real hardware with a client polling like an open window: 13 488 requests, 0 errors, worst latency 13.1 ms; 2 threads and 14 descriptors throughout; memory 5.3–7.9 MiB with no trend; no monitoring interruption; a real `battery_low` alert raised and resolved.
- **Tests**: 342 Rust and 57 frontend tests at the start; 371 Rust and 66 frontend tests at the end, all passing;
  nothing removed or weakened.
- **Status: PARTIALLY COMPLETE.** Everything that could be done on this laptop is done; what
  needs other hardware, another desktop, a real suspend or a full battery discharge is listed in
  section I and remains outstanding, as the plan requires.

## B. Repository findings

### Starting point

- The original Sentinel concept conversation is **not in the project folder** (searched the
  repository's text, the projects folder, and file names across the filesystem four levels
  deep). As in Phase 1, `plan.md` and the
  existing implementation and documents were used as the source of truth.
- Baseline on 2026-11-19 before any change: `cargo test` 342 passed, 0 failed; `npm test` 57
  passed; nothing committed (the repository has no commits). A snapshot of the tree was kept
  outside the repository before changes.

### Working (verified, not changed)

Daemon lifecycle (single instance, lock, graceful `SIGTERM`, stale socket replacement, recovery
after a crash with `recovered_from_unclean_shutdown`), the IPC server (connection limit, idle
timeout, frame size limit, peer-credential check, hostile input), the client timeouts, CLI
commands and exit codes, configuration validation and persistence, hardware and utility
discovery and its cache, all ten collectors (accuracy confirmed on real hardware), sampling
profiles and power-aware sampling (the long run started on battery in Eco and switched to
Balanced when the charger was connected), the alert state machine, SQLite history (batched writes, roll-ups,
retention, migrations; a corrupt database is left untouched and monitoring continues without
history), the desktop's process separation (closing the window leaves the daemon running and
leaves no helper processes).

### Issue register

| ID | Area | Finding | Severity | Status | Evidence |
|---|---|---|---|---|---|
| P4-01 | Monitoring lifecycle | If the monitor thread did not exit within 5 s of `stop` (a collector in a hung sensor read), the handle was dropped; a later `start` cleared the pending stop and ran a **second monitor** beside the first | High (race) | Fixed: the daemon keeps owning the thread; `start` is refused ("still stopping") until it exits | `a_hung_collector_never_leads_to_a_second_monitor_thread` (FIFO in place of `/proc/loadavg`) |
| P4-02 | Window | The status refresh fetched the overview and the configuration with `Promise.all`; when the configuration timed out on a hung daemon, the overview (which reports the broken connection) was thrown away and the window kept showing **"connected"** | High | Fixed: each answer used on its own | Real window: "NO REPLY" in 8 s (never, before) |
| P4-03 | Window, CLI | A daemon that holds its socket but does not answer was treated as **stopped**: pages said "service is not running", the switch offered to start it (a second start attempt), the monitoring indicator said "Inactive", `sentinel status` printed "Daemon: stopped" | High | Fixed: "NO REPLY"/"not answering", monitoring "Unknown", start disabled, last data marked not live | Real window; `a_frozen_daemon_…` (CLI wording, exit 4); `honesty.test.tsx` |
| P4-04 | Window | Background polls used `setInterval` without waiting: with a slow daemon requests **stacked up** (2 in flight per command observed) and answers could be applied out of order | Medium | Fixed: polls are serialised and coalesced | `reliability.test.ts`; Firefox: 1 in flight (old build: 2) |
| P4-05 | Window | After a failed poll the error banner **stayed after the daemon recovered**, and the last snapshot kept being called "Live" while the daemon did not answer | Medium | Fixed: polling errors clear on the next good poll; failed polls mark data "Not live · Sentinel is not answering"; a stopped daemon drops live data without a banner | `reliability.test.ts`, `telemetry.test.tsx`; real window (no leftover banner) |
| P4-06 | Settings | Unsaved threshold and retention edits were **reset every 5 s** (the configuration is re-read, and the draft was reset on every new object) | Medium | Fixed: reset only when the saved values change | Firefox: edit kept over 12 s (old build: reverted within 5 s) |
| P4-07 | Settings | Emptying a numeric field silently saved **0** (e.g. CPU threshold 0 %) | Low | Fixed: empty fields block Save, with a hint | Firefox (old build: saved 0) |
| P4-08 | Battery | For batteries that report charge (this laptop), the runtime estimate's "average draw" was the average **current in amperes, shown as power** ("average draw 0.5") | Medium | Fixed: always watts (current × voltage), or omitted without a voltage; the UI shows the unit | `charge_reporting_battery_reports_its_draw_in_watts`; live: 0.47 → 5.5 W |
| P4-09 | Alerts | `battery_low` documented as "2 readings" was a 1-s hold, but rules are evaluated every 2–5 s and the battery read every 10–30 s: **one odd battery reading raised the alert** (and notified) | Medium | Fixed: the hold is one battery interval | `tests/rules.rs` (fails on the old hold) |
| P4-10 | Alerts | The notification cooldown was lost on every daemon restart and monitoring off/on: a condition still present **notified again at once** | Medium | Fixed: alerts resolved within the cooldown are remembered from history | Unit test; real bus: no repeat after a restart |
| P4-11 | Monitoring | Sustained-duration checks used the wall clock: after a **suspend**, a condition pending before the sleep counted the whole sleep and could raise immediately after resume | Medium | Fixed: a wall/monotonic divergence over 5 s (sleep or clock change) restarts sampling as after a pause | `sleep_and_clock_changes_are_detected`; physical suspend not tested |
| P4-12 | Window | A history database that could not be opened (corrupt) was **not shown**: charts just said "No data" | Low | Fixed: notice on the Telemetry page | `telemetry.test.tsx`; corrupt and truncated databases tested by hand: daemon runs, file untouched |
| P4-13 | Accessibility | Status labels ("Live · updated", "ON/OFF") and empty-state messages were **not exposed** to assistive technologies by WebKitGTK | Low | Fixed: `role="status"` (not announced on each refresh), accessible names | AT-SPI tree of the real window |
| P4-14 | Tests and docs | `ALERT_ENGINE.md` said `rules.rs` and `health.rs` had tests: **neither had any** | Medium (verification gap) | Fixed for the rules: `tests/rules.rs`, 18 tests covering all 13; `health.rs` still has none of its own (exercised by integration tests), now stated | |
| P4-15 | Power | The window **keeps polling when minimised** on GNOME Wayland (1.4 % CPU, about 100 wake-ups/s): the compositor does not tell the app it is minimised, so the page stays "visible". The documentation said polling stops when hidden | Medium (power) | Documented, behaviour unchanged (see I) | `resources.py` |
| P4-16 | Power | The open window costs **1–2 % of a core, 100–190 wake-ups/s, ~440 MiB**, mostly WebKit rendering threads (garbage-collector helpers, vsync, inter-process queues); payloads are only 17 KB per poll | Info | Investigated and documented; not optimised in this phase | Per-thread breakdown in POWER_EFFICIENCY.md |
| P4-17 | Telemetry | A missing core `/proc` file (`stat`, `meminfo`, `diskstats`, `loadavg`) makes that collector `failing` rather than `unsupported` | Low | Accepted as designed (abnormal on Linux); documented | `machines_without_battery_or_gpu_…` |
| P4-18 | History | SQLite uses a rollback journal: a history query that meets a flush waits (up to 500 ms) on the daemon's single IPC thread | Low | Accepted: flushes are 5 minutes apart and take milliseconds; worst client latency in the long run: 13.1 ms (mean 0.45 ms over 13 488 requests) | Long run |
| P4-19 | Scope | The plan's page list names a **History** page; there is none. History is reached through the Telemetry charts (5 min to 30 days) and the Alerts history | Info | Documented; no page added (no new features in Phase 4) | |
| P4-20 | CLI | A hung daemon gives exit code 4 (IPC failure), not 9 (timeout): the overview does not keep the error kind | Low | Accepted, documented | |
| P4-21 | Tests | A reliability test that froze its daemon and then failed **left a stopped daemon behind** (found during Phase 4 development) | Low (test hygiene) | Fixed: a guard resumes the daemon even when the test fails | Two leaked test daemons cleaned up |
| P4-22 | IPC | The answer to a sampling-profile change reported the profile in effect **before** the change until the monitor thread woke up: under load, turning power-aware sampling off answered "effective: Eco" (the CLI printed it). Found when the Phase 3 test `power_aware_sampling_follows_the_power_source` failed once in the final regression run | Low (race) | Fixed: the answer is computed exactly as the monitor will decide it | That test, 3 further runs plus the full suite: pass |
| P4-24 | History | **Write amplification**: each 5-minute flush writes up to about 1 MB for about 20 KB of data (rows ordered by series touch a page per series; the rollback journal doubles it): 11.6 MiB/h, about 280 MiB a day on disk while monitoring. Phase 3's documentation had explained the absence of device writes by the page cache; the real reason was that its test home was on `tmpfs` | Low (efficiency) | Investigated and documented with measured options (WAL 7.1 MiB/h, 15-min flush 4.7, both 3.2); **not changed**, each option trades durability or storage behaviour (decision for the user). Phase 3 text corrected | Probe on ext4; long run |
| P4-23 | Build | The documented release build `npm run desktop:build` **failed at packaging** ("invalid category": `System` is not a Tauri category); Phase 3 had used `--no-bundle` | Medium (build) | Fixed: category `Utility`; the package description updated (it still described the Phase 1 foundation). The `.deb` builds and contains `sentinel`, `sentineld` and `sentinel-desktop` | `npm run desktop:build` |

### Potential issues checked and not found

No resource leak in the daemon over connection churn or the long run (descriptors and threads
constant; memory flat, see F); no unbounded database growth (retention enforced; growth rate in
F); no duplicate collectors after mode switches, monitoring restarts, daemon restarts or window
restarts; no blocked window during IPC faults; no excessive notifications; unsupported hardware
reported as such.

## C. Implementation changes

No file was removed. Changed relative to the start of Phase 4:

**Daemon and engine**
- `crates/sentinel-daemon/src/monitoring.rs`: keep owning a monitor thread that misses the stop
  deadline (P4-01); detect sleep and clock changes and restart sampling (P4-11); remember
  resolved alerts within the cooldown at start (P4-10); tests for both.
- `crates/sentinel-alerts/src/evaluator.rs`: `remember_resolved` (P4-10), test.
- `crates/sentinel-alerts/src/rules.rs`: `battery_low` hold of one battery interval (P4-09).
- `crates/sentinel-telemetry/src/runtime.rs`, `collectors/battery.rs`, `model.rs`: average draw
  in watts or none (P4-08).
- `apps/cli/src/commands.rs`: "not answering" instead of "stopped" (P4-03), test.
- `crates/sentinel-daemon/src/telemetry_requests.rs`: the profile-change answer (P4-22).
- `apps/desktop/src-tauri/tauri.conf.json`: package category and description (P4-23).
- `crates/sentinel-daemon/Cargo.toml`: `rustix` with `fs` as a dev-dependency (FIFOs in tests;
  already a workspace dependency, no new crate; `Cargo.lock` gains one dependency edge).

**Desktop frontend**
- `services/serialise.ts`, `hooks/useSerialised.ts` (new): non-overlapping polls (P4-04).
- `app/pollState.ts` (new): what a poll result does to the display (P4-05).
- `hooks/useTelemetry.ts`, `hooks/useSentinel.ts`: use both; `Promise.allSettled` (P4-02);
  unknown service state when unreachable (P4-03); stale marking.
- `app/App.tsx`, `pages/OverviewPage.tsx`, `pages/telemetry/TelemetryPage.tsx`,
  `components/Header.tsx`, `app/viewModel.ts`: unreachable and stale states, history notice
  (P4-03, P4-05, P4-12), "NO REPLY", "Unknown".
- `pages/SettingsPage.tsx`: drafts and empty fields (P4-06, P4-07).
- `components/primitives.tsx`: accessible empty states (P4-13).
- `app/telemetryView.ts`, `types/telemetry.ts`: watts, nullable draw (P4-08).

**Tests (new or extended)**
- `crates/sentinel-alerts/tests/rules.rs` (new, 18 tests).
- `tests/integration/reliability.rs` (new, 6 tests); `tests/integration/telemetry.rs` (helpers
  shared); `tests/integration/main.rs` (module).
- `crates/sentinel-telemetry/tests/fixture_collectors.rs` (charge-reporting battery).
- `apps/desktop/src/app/reliability.test.ts` (new), `telemetry.test.tsx`, `honesty.test.tsx`.
- `tests/desktop-e2e/` (new): real-window scenarios, notifications, accuracy, resources, long
  run, browser checks, with a README. Manual; not part of `cargo test`.

**Documentation**: new `PHASE_4.md`, `DESKTOP_INTEGRATION.md`, `IPC_RELIABILITY.md`,
`NOTIFICATION_VALIDATION.md`, `HARDWARE_COMPATIBILITY.md`; updated `README.md`,
`DEVELOPMENT.md`, `ALERT_ENGINE.md`, `TELEMETRY_ENGINE.md`, `METRIC_REFERENCE.md`,
`MONITORING_LIMITATIONS.md`, `POWER_EFFICIENCY.md`.

**IPC**: no protocol change; `average_power_w` is now watts and nullable (see IPC_RELIABILITY.md).

## D. Reliability improvements

- **Daemon**: at most one monitor thread, ever (P4-01); sleep and clock changes no longer skew
  alerts or rates (P4-11).
- **IPC and window**: bounded, non-overlapping requests (P4-04); three distinct, truthful states
  for running, stopped and not answering, with no second-daemon attempt (P4-02, P4-03); errors
  that clear themselves after recovery and data that is never called live when it is not
  (P4-05).
- **Monitoring**: correct battery draw (P4-08); missing history is visible (P4-12).
- **Alerts**: no low-battery alert from one reading (P4-09); no repeated notifications after a
  restart (P4-10); all 13 rules tested individually (P4-14).
- **Database**: verified rather than changed: corrupt or truncated `history.db` → monitoring
  continues without history, the file is not modified or deleted; growth bounded by retention.
- **Settings**: edits are not lost; empty fields are not saved; invalid values are refused with
  the file unchanged (tested).

## E. Hardware validation

Physically tested: the development laptop only (AMD Ryzen 3 7320U, Radeon 610M, NVMe, Wi-Fi,
charge-reporting battery, dell_smm fan and sensors; Ubuntu 26.04, GNOME 50.1 Wayland). CPU, memory,
swap, storage, network, temperatures, GPU (amdgpu), battery levels and states, processes and
notifications were checked against the kernel or the service. A real `battery_low` alert was
raised at 15 % while discharging during the long run.

Unsupported here and therefore not physically tested: Intel and NVIDIA GPUs, discrete AMD GPUs,
energy-reporting batteries, the full-charge state, desktops without a battery, CPU-only machines,
KDE Plasma, X11, Ethernet, SATA and USB disks, and real suspend. All except suspend and KDE have
fixture tests. Full matrix: [HARDWARE_COMPATIBILITY.md](HARDWARE_COMPATIBILITY.md).

## F. Resource measurements

Final build, on mains power, two runs (details and method: [POWER_EFFICIENCY.md](POWER_EFFICIENCY.md)).

| What | CPU (% of one core) | Wake-ups/s | Memory |
|---|---|---|---|
| Daemon, monitoring off | 0.000 | 0 | 5.8–6.1 MiB, 1 thread |
| Daemon, Eco | 0.14 | 1.0 | 7.0 MiB, 2 threads |
| Daemon, Balanced (default) | 0.28–0.30 | 2.2–2.3 | 7.2–7.3 MiB |
| Daemon, Performance | 0.56–0.58 | 5.1 | 7.3–7.4 MiB |
| Window open on the Overview, monitoring on | 1.9–3.0 | ≥ 130–260 | 437–453 MiB (3 processes) |
| Window on Telemetry (charts) | 2.1–4.1 | ≥ 190–450 | 441–456 MiB |
| Window open, monitoring off | 0.7–0.8 | ≥ 56 | about 440 MiB |
| Window minimised, monitoring on | 1.4–1.9 | ≥ 95–104 | 440–479 MiB |

The daemon meets the plan's aims (no polling while off, low idle cost, no GPU, no growth). The
window is the expensive part, because of WebKit rendering (P4-16), and it keeps polling when
minimised on Wayland (P4-15).

Database activity: in the long run the database grew 337 KiB an hour (13 677 rows after 2.5 h; bounded by retention), with one write burst per 5-minute flush and under 1 KiB a minute otherwise; the bytes written per flush grow with the file (P4-24). The run's home was on `tmpfs`, so device writes were measured separately on ext4: 11.6 MiB an hour.

Power impact on battery: **not measured.** The laptop was on battery for part of the long run (it
reached 15 %; the charger was then connected), but a valid comparison needs hours of controlled
discharge with monitoring on and off. No battery-life claim is made.

## G. Test results

| Suite | Start of Phase 4 | End of Phase 4 | Passed | Failed | Skipped |
|---|---|---|---|---|---|
| Rust (`cargo test`, unit + integration + hardware + safety + doc) | 342 | 371 | 371 | 0 | 0 |
| Frontend (`npm test`) | 57 | 66 | 66 | 0 | 0 |
| Real window (`window_scenarios.py`) | none | 12 | 12 | 0 | 0 |
| Browser (`browser_settings.mjs`) | none | 7 | 7 | 0 | 0 |
| Notifications on the real bus (`notifications.py`) | none | 7 steps | 7 | 0 | 0 |
| Accuracy (`accuracy.py`) | none | 31 | 30 + 1 both unavailable | 0 | 0 |

New Rust tests: 29 (18 alert-rule tests, 6 reliability integration tests, 2 monitoring unit
tests, 1 evaluator test, 1 battery fixture test, 1 CLI test). New frontend tests: 9 (6 for polling and its error states, 2 telemetry-page states, 1 unreachable-daemon indicator). No
existing test was deleted or weakened. Existing tests changed only where a type changed (two
assertions now unwrap `average_power_w`, which became optional) or gained assertions (the
runtime-text test now checks the watt unit), and the Phase 3 integration helpers were made
shareable with the new reliability module.
`cargo fmt --check`, `cargo clippy --all-targets -D warnings` (desktop included), `tsc` and the
production build are clean. Regression findings: none (every Phase 1–3 test still passes).

## H. Safety verification

| Boundary | Audited | Violations |
|---|---|---|
| No OS modification (governor, GPU power, power profiles, BIOS, kernel parameters, systemd, packages, drivers, network) | Phase 4 source changes reviewed; the safety suite (20 tests: read-only code paths, forbidden operations, IPC surface, desktop command list) passes unchanged | None |
| No automatic fixes or maintenance execution | Alerts still carry text-only recommendations; notifications register no actions | None |
| No uploads; no new network access | No new dependency; no network code | None |
| Tests read-only | Every test and script ran Sentinel in a throwaway home; only Sentinel's own test daemons were signalled; no setting was changed, nothing was stressed, no memory was exhausted, the laptop was not suspended; controlled alerts were produced by raising thresholds, never by load | None |
| Privacy | Logs and reports contain no user data beyond hardware identifiers already shown by Phase 2 | None |

## I. Outstanding issues

Not fully verified (needs hardware, an environment or time not available here):

1. **Real suspend and resume** (window open, monitoring on). The handling is new and unit
   tested only.
2. **Battery runtime accuracy and Sentinel's battery impact**: a full, controlled discharge.
3. **Full-charge state**, **energy-reporting batteries**, **desktop without a battery**,
   **CPU-only** machines, **Intel/NVIDIA/discrete AMD GPUs**, **KDE Plasma**, **X11**,
   **16 GB** machines, **Ethernet**, **SATA/USB disks**: fixture tests only, or none.
4. **Typing in the WebKit window**: not possible through AT-SPI; the same frontend was tested
   in Firefox.

Known issues left as they are:

5. **The window polls while minimised on Wayland** (P4-15). A fix would lower the live refresh
   rate while the window is not focused; that changes what someone watching it on a second
   screen sees, so it is a product decision. Until then: close the window when it is not needed
   (the daemon keeps running) or turn monitoring off.
6. **The window's own cost** (P4-16): WebKit rendering. Candidates for a later phase: skip
   re-rendering unchanged panels, lighter chart updates.
7. **History write amplification** (P4-24): about 280 MiB a day written while monitoring.
   Options, measured: WAL journal (−40 %), a 15-minute flush (−60 %, but up to 15 minutes of
   history lost on a crash), both (−72 %), or a time-ordered table (schema migration). A
   decision for the user; nothing changed.
8. P4-17, P4-18, P4-20 (accepted, documented).

## J. Phase completion status

**PARTIALLY COMPLETE.** Every criterion that can be verified on this laptop has been verified;
the physical validations in section I remain outstanding and are not marked as done.

| Definition of done | Status |
|---|---|
| All existing pages work; navigation reliable | Done (real window, 3 rounds; Firefox) |
| UI reflects daemon state; loading and error states | Done (stopped, frozen, crashed, restarted) |
| No critical frontend exceptions | Done (Firefox console clean; nothing observed in the window) |
| Startup and shutdown reliable | Done |
| Daemon starts; UI connects to an existing daemon; survives disconnection; reconnects; no duplicate collectors on restart; IPC errors handled | Done |
| All collectors tested; units; timestamps; unavailable metrics honest | Done (fixtures + real hardware) |
| Monitoring modes work; settings persist; no unnecessary consumption | Modes and settings: done. Consumption: the daemon's is low; the window's cost (P4-15, P4-16) and history write amplification (P4-24) are documented, not reduced |
| All 13 alert rules verified; sustained checks; deduplication | Done (`tests/rules.rs`) |
| Desktop notifications physically verified where supported | Done on GNOME; KDE not available |
| SQLite records consistent; history survives restarts | Done |
| Battery telemetry verified where available | Done for this battery (charge-reporting) |
| Runtime estimation investigated | Done (investigated, one defect fixed); accuracy over a real discharge outstanding |
| GPU discovery verified | AMD integrated: done. Intel/NVIDIA: fixtures only (outstanding) |
| Unsupported hardware handled; CPU-only supported | Done with fixtures; physical outstanding |
| Sleep/resume investigated | Investigated and hardened; physical test outstanding |
| Extended monitoring tested | Done: 2.5 hours, no leak, no error (POWER_EFFICIENCY.md) |
| No critical memory leaks; no uncontrolled database growth; no critical crash or recovery issues | Done (growth bounded by retention; write volume is P4-24) |
| Safety (no OS changes, no automatic fixes, no maintenance execution, no uploads, read-only testing) | Done |
| Existing and new tests pass; builds; no Phase 1–3 regression | Done |

## K. Next development phase

Phase 5 (Hardware Explorer and Component Visualisation) builds on what Phase 4 made dependable:

- The **inventory** (Phase 2) is the data model for an explorer; Phase 4 confirmed it on real
  hardware and fixtures, and the matrix says which component kinds have only fixture coverage.
- **Live readings per component** (temperatures, GPU, storage, battery) are now known to be
  correct and honestly marked when absent, so a component view can show them next to the
  inventory without re-validating them.
- The **window's data flow** (one serialised poller, stale and unreachable states, accessible
  status labels) is the pattern new pages should reuse rather than polling on their own.
- The **end-to-end harness** (`tests/desktop-e2e`) can drive new pages in the real window from the
  start.
- Constraints to carry forward: the window's rendering cost (P4-16) argues for static or lightly
  updated diagrams; the explorer must stay read-only.

Phase 5 has not been started.
