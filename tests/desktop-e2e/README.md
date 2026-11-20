# Desktop end-to-end checks (Phases 4, 5 and 6)

These scripts drive the **real** Sentinel binaries and the **real** desktop window on a Linux
desktop session. They complement `cargo test` and `npm test`; they are not part of either,
because they need a graphical session, open a visible window, and take minutes.

Every script runs Sentinel in a throwaway home directory with its own runtime directory, so your
own Sentinel data is never read or written. They read the real `/proc` and `/sys` (read-only, as
Sentinel always does). Signals (`SIGSTOP`, `SIGCONT`, `SIGKILL`) are sent only to the daemon the
script itself started, to simulate a hung or crashed daemon; Sentinel itself never signals
anything. No script changes a system setting, stresses the hardware or needs root.

## Requirements

- A built release: `cargo build --release -p sentinel-daemon -p sentinel-cli` and the desktop app
  built with `npm run desktop:build` (or `npm run tauri -w apps/desktop -- build --no-bundle`).
  A plain `cargo build -p sentinel-desktop` gives a window that loads the development server and
  shows nothing; it cannot be tested this way.
- Python 3 with PyGObject and AT-SPI (`gi.repository.Atspi`), present on GNOME and most desktops,
  and the accessibility bus (on by default on GNOME).
- `dbus-monitor` for the notification check.

## Scripts

| Script | What it checks | Visible effect |
|---|---|---|
| `window_scenarios.py <repo> <workdir>` | 12 scenarios in the real window: startup, navigation, telemetry tabs, monitoring and profile controls, settings persistence, daemon stopped / restarted / frozen / crashed while the window is open, an alert raised, acknowledged and resolved, closing and relaunching | The Sentinel window opens for about 3 minutes. Notifications are off. |
| `explorer_scenarios.py <repo> <workdir> [seconds]` | Phase 5 hardware explorer: every category renders; the CPU tree, core threads and L3 sharing match sysfs; live per-core load in the inspector; selection kept across navigation and refreshes; root partition and filesystem match `/proc/mounts`; PCI and USB counts match sysfs; GPU→PCI link; battery capacities match sysfs in the battery's own unit; MAC addresses never unmasked; then the window's CPU and memory on five pages for `seconds` each (`0` skips the measurement) | The Sentinel window opens for about `5 × seconds` + 1 minute. Notifications are off. |
| `docs_scenarios.py <repo> <workdir> [--online] [--real-download] [--web-catalogue <https url>]` | Phase 6 Schematic Library: the identity shown equals `/sys/class/dmi/id`; an offline search opens no TCP/UDP socket and finds a synthetic library PDF as an exact match; the detail panel shows the evidence and the source separately; a second search reuses the cache; clearing keeps the library; the explorer's motherboard category links to it. `--online` repeats a search through the real daemon and the real Internet Archive with the *fixture* identity (run `SENTINEL_FIXTURE_DUMP=<workdir>/fixture-root cargo test -p sentinel-tests --test hardware -- --ignored dump_amd` first); this computer's identity is never sent. `--real-download` builds a ThinkPad T480 fixture and downloads Lenovo's official PSREF PDF from the window through the daemon (from a local catalogue), then removes it. `--web-catalogue` does the same from a valid catalogue at a real HTTPS address (for example `https://httpbin.org/base64/<base64url of the catalogue JSON>`) | The window opens for about a minute (two minutes with `--online`) |
| `notifications.py <repo> <workdir>` | A controlled alert (low-memory threshold raised to 99 %) is detected, stored, sent to the real notification service exactly once, shown by GNOME Shell, not repeated while active, on recovery, inside the cooldown or after a daemon restart | **One real desktop notification**, "Low available memory". Your memory is not affected. |
| `node browser_settings.mjs [dist]` | The built frontend (`npm run build`) in headless Firefox with a scripted bridge: every page renders with a clean console, an idle window polls once per period, an unsaved Settings edit survives refreshes, an empty field cannot be saved, requests never overlap against a 9-second daemon. Pass an older `dist` to compare builds | None (headless). With snap Firefox the temporary profile is created, and removed, under `~/snap/firefox/common` |
| `accuracy.py <repo> <workdir> <seconds>` | Sentinel's readings against the kernel's: CPU (total and per core), network, disk, memory, swap, battery, temperatures, GPU, process | None |
| `resources.py <repo> <workdir> <seconds>` | Sentinel's own CPU, wake-ups (per thread), memory and writes: the daemon per mode and the window in several states | The window opens for part of the run |
| `longrun.py <bin_dir> <home> <rundir> <out.jsonl> <seconds>` | A long monitoring session with a client polling like an open window; one JSON line per minute | None |

`ui.py` is the small AT-SPI helper the window scripts use: it finds the window, reads its
accessible text and invokes buttons' default actions. It never synthesises keyboard input, so
nothing can be typed into another application.

Results from the Phase 4 runs are in `docs/DESKTOP_INTEGRATION.md`,
`docs/NOTIFICATION_VALIDATION.md` and `docs/POWER_EFFICIENCY.md`.

## Limits

- Text fields cannot be filled through AT-SPI in WebKitGTK (no editable-text interface), so
  typing into Settings is tested in Firefox (`browser_settings.mjs`), not in the WebKit window.
- `notifications.py` looks for the banner in GNOME Shell's accessibility tree; on other desktops
  it reports the delivery result only.
