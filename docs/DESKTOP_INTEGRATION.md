# Desktop integration test report (Phase 4)

What was tested in the real desktop application and on real hardware, how, and the result.
Daemon and IPC fault handling has its own report: [IPC_RELIABILITY.md](IPC_RELIABILITY.md).
Notifications: [NOTIFICATION_VALIDATION.md](NOTIFICATION_VALIDATION.md).

## Environment

| | |
|---|---|
| Machine | Dell Inspiron 15 3535 laptop: AMD Ryzen 3 7320U (4 cores, 8 threads), 6.5 GiB RAM, Radeon 610M integrated GPU, NVMe SSD, charge-reporting Li-ion battery |
| OS | Ubuntu 26.04, kernel 7.0, GNOME 50.1 on Wayland |
| Build | Release binaries; desktop app built with `npm run desktop:build` (Tauri 2, WebKitGTK; also produces the `.deb`) for the final runs, `--no-bundle` for earlier ones |
| Isolation | Every run used a throwaway `HOME` and runtime directory; the real `/proc` and `/sys` were read |
| Date | 2026-11-19 |

## How the real window was driven

`tests/desktop-e2e/window_scenarios.py` launches the real `sentinel-desktop` binary and drives it
through the accessibility tree (AT-SPI): it reads the page text and invokes the default action of
buttons, switches, radio buttons and tabs, exactly what a click does. It never synthesises
keyboard input, so nothing could be typed into another application. Every check of "what the
window shows" is confirmed against the daemon (`sentinel status`/`monitor status`, IPC) or the
configuration file.

WebKitGTK cannot fill text fields through AT-SPI, so typing (the Settings thresholds) is tested
in a real browser engine instead: `tests/desktop-e2e/browser_settings.mjs` runs the built
frontend in headless Firefox with a scripted bridge.

## Results in the real window

All 12 scenarios passed on the final build, and also on the build just before the last daemon fix
(P4-22). See "Harness corrections" for the failed first runs and why.

| Scenario | Result | Measured |
|---|---|---|
| Start the window while the daemon is already running | Pass | Live data shown 2.2 s after launch; header "Monitoring Active", service "ON" |
| Navigation: 3 rounds over all 10 pages, each page's own heading checked, no "undefined"/"NaN"/"[object Object]" | Pass | 30 navigations, mean 0.21 s, slowest 0.82 s |
| Telemetry: all 8 tabs; chart windows 15 min, 1 h, 24 h, 5 min | Pass | Every tab's own content appears; the 5-minute CPU chart has data |
| Monitoring off and on from the header switch | Pass | The daemon agrees each time; Overview says "Monitoring off · no live data" while off |
| Sampling profile Eco / Performance / Balanced from the Overview | Pass | Daemon CPU interval 5000 / 1000 / 2000 ms; `config.toml` updated each time; one monitor thread throughout |
| Settings: notifications switch, theme | Pass | Written to `config.toml` and applied by the daemon (its notification status follows) |
| Daemon stopped (`sentinel stop`) while the window is open | Pass | 0.7–3.6 s later (within one 5-s refresh): header "OFF", monitoring "Inactive", Telemetry "The Sentinel service is not running", no live values, no error banner |
| Daemon restarted while the window is open | Pass | Live again 4.9 s after the restart (within one 5-s refresh); monitoring resumed by preference; no stale banner |
| Daemon frozen (`SIGSTOP`, every request times out), then resumed | Pass | Header "NO REPLY" and monitoring "Unknown" after 7.9–8.0 s (5-s refresh + 3-s timeout); Overview "Not live · Sentinel is not answering"; service switch disabled (no second daemon can be started); navigation still answers in 0.3 s; desktop threads 38 → 39 (no pile-up); live again 0.3 s after `SIGCONT`; no leftover error |
| Daemon killed (`SIGKILL`), then started from the window's switch | Pass | "OFF"/"Stopped" at once; started in 0.7 s; notice "The previous Sentinel session did not shut down cleanly"; one monitor thread |
| Alert raised, acknowledged, resolved | Pass | A safe controlled alert (free-space threshold raised to 99 %): 2 storage alerts (one per writable local filesystem) appear on the Alerts page; "Acknowledge" marks one as seen in the daemon; after restoring the threshold the page says "No active alerts" and lists them under history |
| Close the window with its close button, relaunch | Pass | The window and its 2 WebKit helper processes exit; the daemon keeps monitoring; the relaunched window reconnects; still 2 daemon threads |

### In a browser engine (headless Firefox, scripted bridge)

| Check | Result |
|---|---|
| Every page renders, twice, with no uncaught exception or console error | Pass: 20 page views, console clean |
| An idle window polls once per period, not per component | Pass: 4 overview, 4 configuration, 4 metrics calls in 20 s (5-s period while monitoring is off) |
| An unsaved threshold edit survives the configuration being re-read | Pass (the pre-Phase-4 build fails: the field reverted to 90 within 5 s) |
| An emptied field blocks Save instead of saving 0 | Pass (pre-Phase-4 build: became 0 and could be saved) |
| Saving sends exactly what was typed | Pass |
| With a daemon taking 9 s per answer, no command has two requests in flight | Pass: at most 1 (pre-Phase-4 build: 2) |

## Workstream B checklist

| Requirement | Status | Evidence |
|---|---|---|
| Every navigation item opens the correct page, repeatedly, without refreshing | Verified (real window) | Navigation scenario |
| No stale information after navigation | Verified | Each page's heading and content checked after each click; pages share one poller, so they show the same snapshot |
| Loading, empty and error states | Verified | Stopped, frozen and crashed daemon scenarios; empty states ("No active alerts", "service is not running") |
| No uncaught frontend exceptions | Verified in Firefox (console captured). Not observable in the WebKit window (no inspector in a release build) |
| No broken IPC requests | Verified: no error banner in any normal scenario; the daemon logged no rejected request |
| No unnecessary duplicate fetching | Verified: one poll per period; pages do not fetch independently except charts (history) and the process table, each only while shown |
| Launch, connection, initial state, hardware, monitoring status, settings, history | Verified (startup scenario, telemetry charts) |
| Startup failures handled | Daemon not running: the window opens, says "OFF"/"Stopped", offers Start. Desktop libraries missing: `sentinel ui` reports it (Phase 1 test) |
| Never pretends to monitor while disconnected | Verified: stopped → "Inactive"; frozen → "Unknown"/"Not live"; data dropped or marked stale |
| Shutdown leaves no processes; daemon lifecycle unchanged; no duplicate collectors on UI restart | Verified (close and relaunch scenario) |
| Database writes not interrupted by closing the window | By design: the window never writes the database; the daemon flushes on its own schedule and on shutdown |

## Measurement accuracy on real hardware

`tests/desktop-e2e/accuracy.py`, Performance profile. Rates: the mean of Sentinel's live series
over 240 s against the kernel counter difference over the same 240 s, computed independently
with the same definition. Levels: Sentinel's snapshot against a direct read right after it.

| Metric | Sentinel | Kernel | Source |
|---|---|---|---|
| CPU total | 9.77 % | 9.74 % | `/proc/stat` delta, 239 samples |
| CPU per core, worst of 8 | 0.07 points off | | `/proc/stat` per-CPU deltas |
| CPU total vs mean of cores | 9.774 % | 9.775 % | Sentinel's own series (aggregation) |
| Network wlp2s0 receive / send | 56 215 / 3 220 B/s | 55 976 / 3 211 B/s | `/proc/net/dev` |
| Disk nvme0n1 read / write | 56 735 / 110 023 B/s | 57 002 / 112 827 B/s | `/proc/diskstats` |
| Memory total | 6 940 008 448 B | identical | `/proc/meminfo` |
| Memory available | 3170.2 MiB | 3170.7 MiB | `MemAvailable` |
| Memory used | 52.1 % | 52.09 % | `MemTotal − MemAvailable` |
| Swap used | 2241.5 MiB | identical | `SwapTotal − SwapFree` |
| Battery charge level, voltage, charge, state | 50 %, 12.14 V, 1.49 Ah, charging | 50 %, 12.143 V, 1.494 Ah, charging | `power_supply/BAT1` |
| 8 temperature sensors (k10temp, dell_smm ×3, nvme ×2, amdgpu, ACPI zone) | within 0.75 °C | | each sensor's `temp*_input` |
| Battery hwmon temperature | not provided | the kernel returns `ENODEV` | Correctly reported as not available, not as a number |
| GPU VRAM / GTT used, temperature | 448.9 MiB / 268.6 MiB / 59 °C | 448.9 / 266.6 / 59 | `mem_info_*`, hwmon |
| Process RSS and threads (sentineld) | 7.1 MiB, 2 | 7.7 MiB, 2 | `/proc/PID/status` (RSS moves between the two reads) |

Units, timestamps (wall-clock milliseconds; rates stamped at the end of their interval) and
unavailable values (state + quality, never a substituted zero) were checked in the same run and
by the fixture tests.

The battery's own runtime estimate could not be checked against a real discharge: the laptop was
charging for most of the session. During the part on battery (15 %, discharging at 0.47 A) the
estimate was 58 min, which equals remaining charge / average current (0.46 Ah / 0.47 A). That it
showed the current in place of watts ("average draw 0.5") was found here and fixed.

## Harness corrections

The first runs failed because of the harness, not the app: WebKit exposes text after CSS
`text-transform` (so "ECO", not "Eco"), and loose text in generic containers is not exposed at all.
The latter is also an accessibility defect for screen-reader users, so the status labels ("Live ·
updated", "ON/OFF/NO REPLY") and empty-state messages now carry `role="status"` (not announced on
every refresh) and empty states carry their title as an accessible name.

## Not tested

- KDE Plasma and other desktops (only GNOME 50.1 was available).
- X11 sessions (Wayland only).
- Typing in the WebKit window itself (see above; tested in Firefox).
- Window behaviour across a real suspend and resume (no suspend happened during testing).
