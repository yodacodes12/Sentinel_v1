# Sentinel

Sentinel is a Linux system intelligence hub: one place to understand your computer. It is made of
a lightweight background daemon, a command-line interface and a native desktop application.

**Sentinel observes, analyses and recommends. It never changes your system.** It does not install
packages, edit system files, change power or network settings, or stop your processes. See
[docs/SAFETY_POLICY.md](docs/SAFETY_POLICY.md).

## Current implementation status

This repository is at **Phase 6: automatic schematic discovery and verification**. The
Schematic Library searches your document library (and, if you turn on online discovery, public
sources) for documentation of the detected motherboard, and says for each result how well it
matches the board, with the evidence, separately from who published it
([docs/DOCUMENT_DISCOVERY.md](docs/DOCUMENT_DISCOVERY.md), [docs/PHASE_6.md](docs/PHASE_6.md)).
Online discovery is **off by default**; with it off Sentinel makes no network request.

Phase 5 is **hardware explorer and component visualisation**. The Hardware
page is now an interactive explorer of the discovered hardware: component trees, logical
diagrams (core and cache topology, memory capacity, GPU functional blocks, disk layout, battery
capacity, USB and PCI hierarchies), live readings and an inspector that shows where every value
came from ([docs/HARDWARE_EXPLORER.md](docs/HARDWARE_EXPLORER.md), [docs/PHASE_5.md](docs/PHASE_5.md)).
Phase 4, desktop integration and reliability, is described in [docs/PHASE_4.md](docs/PHASE_4.md).

| Area | Status |
|---|---|
| Background daemon (`sentineld`) | Working: lifecycle, status, configuration, scan coordination, monitoring |
| CLI (`sentinel`) | Working: `start`, `stop`, `status`, `ui`, `logs`, `scan`, `hardware`, `utilities`, `monitor`, `metrics`, `alerts` |
| IPC over a private Unix socket | Working: versioned, typed, validated |
| Hardware discovery (read-only) | Working: identity, firmware, board, CPU topology, memory, GPU, storage, battery, network, USB, PCI, sensor interfaces |
| Utility and service discovery (read-only) | Working: desktop entries, dpkg/RPM/pacman/Flatpak/Snap databases, `PATH`, systemd listings |
| Inventory cache | Working: versioned, in the local database; shown as "cached" when not from this session |
| Telemetry engine (read-only) | Working: CPU and per-core, memory and swap, processes, battery, temperatures, fans, GPU (where the driver exposes it), storage I/O and capacity, network, load and pressure |
| Sampling profiles | Working: Eco, Balanced, Performance; optional power-aware switch to Eco on battery |
| Alerts and health indicators | Working: sustained conditions with hysteresis, cooldown and duplicate suppression; evidence-based health per area |
| Desktop notifications | Working where a freedesktop notification service is running (session D-Bus); physically verified on GNOME 50.1 |
| Telemetry history | Working: local SQLite, 30-second detail and hourly roll-ups, retention enforced |
| Desktop frontend (React) | Working: Overview with live tiles, Telemetry (CPU, Memory, Processes, Battery, Thermal, GPU, Storage, Network), Alerts, Settings, Hardware explorer, plus the Phase 2 pages |
| Hardware explorer | Working: overview, per-category component trees, logical diagrams, live readings, inspector with provenance; logical topology only, no physical layout |
| Desktop shell (Tauri) | Working: compiles and runs (needs the WebKitGTK development packages, see Prerequisites) |
| Documentation discovery | Working: local library offline; opt-in online search (Internet Archive, user-configured catalogues); hardware-match verification with evidence; separate source trust; opt-in, checked downloads |
| Diagnostics, schematic parsing, repair | **Not implemented** (later phases; Sentinel will never repair anything itself) |

**Monitoring is off until you turn it on** (`sentinel monitor start`, or the switch in the desktop
app). While it is off, nothing is sampled: the daemon sleeps until something talks to it. Scans
still run only when you ask (`sentinel scan` or the Scan button).

How the telemetry works: [docs/TELEMETRY_ENGINE.md](docs/TELEMETRY_ENGINE.md). Every metric, its
source and unit: [docs/METRIC_REFERENCE.md](docs/METRIC_REFERENCE.md). Alerts:
[docs/ALERT_ENGINE.md](docs/ALERT_ENGINE.md). Measured overhead:
[docs/POWER_EFFICIENCY.md](docs/POWER_EFFICIENCY.md). What cannot be measured:
[docs/MONITORING_LIMITATIONS.md](docs/MONITORING_LIMITATIONS.md). What was tested on which
hardware: [docs/HARDWARE_COMPATIBILITY.md](docs/HARDWARE_COMPATIBILITY.md). Behaviour when the
daemon is missing, slow or crashes: [docs/IPC_RELIABILITY.md](docs/IPC_RELIABILITY.md). Known
issues: [docs/PHASE_4.md](docs/PHASE_4.md#i-outstanding-issues) and
[docs/PHASE_5.md](docs/PHASE_5.md#j-outstanding-limitations).

## Prerequisites

- Linux (developed on Ubuntu 26.04, GNOME, Wayland)
- Rust stable 1.89 or newer, with Cargo
- Node.js 20 or newer, with npm
- A C compiler (`build-essential`); SQLite is compiled in, no system package needed
- For the desktop shell only, the Tauri 2 Linux dependencies:

  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev
  ```

  The daemon, CLI and all Rust tests build and run without these.

## Development setup

```bash
git clone <this repository> sentinel && cd sentinel
npm install            # frontend dependencies (versions pinned by package-lock.json)
cargo build            # daemon, CLI and libraries (versions pinned by Cargo.lock)
```

## Build commands

```bash
cargo build                         # debug build of sentineld and sentinel
cargo build --release               # optimised build
npm run build                       # type-check and bundle the frontend
npm run desktop:build               # release daemon + CLI, then the desktop app and .deb
```

`cargo build` deliberately skips the desktop shell so it works without the WebKitGTK packages.
Build the shell with the npm scripts (`npm run desktop:build`, or
`npm run tauri -w apps/desktop -- build --no-bundle` to skip the `.deb`). A plain
`cargo build -p sentinel-desktop` compiles, but without Tauri's `custom-protocol` feature the
window loads the dev server (`http://127.0.0.1:1420`) instead of the bundled frontend, so it only
works together with `npm run desktop:dev`.

## Run commands

```bash
./target/debug/sentinel start       # start the daemon as your user
./target/debug/sentinel status
./target/debug/sentinel scan        # read-only hardware and utility scan
./target/debug/sentinel hardware    # the inventory (live from this session, or cached)
./target/debug/sentinel utilities   # installed system utilities
./target/debug/sentinel monitor start   # turn monitoring on (remembered across restarts)
./target/debug/sentinel metrics     # live measurements
./target/debug/sentinel alerts      # active alerts
./target/debug/sentinel monitor stop
./target/debug/sentinel stop

npm run desktop:dev                 # desktop app with live reload
npm run dev                         # frontend only, in a browser (shows "preview mode")
```

The daemon can also be run in the foreground: `./target/debug/sentineld --foreground`.

An optional systemd user unit is provided in `resources/systemd/sentinel.service`. Sentinel never
installs or enables it; the file explains how to do so yourself.

## Test commands

```bash
cargo test                          # unit, integration and safety tests
npm test                            # frontend tests
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

The integration and safety tests run the real binaries inside a temporary directory acting as
`HOME`. They never touch your own Sentinel data or anything else on the machine.

End-to-end checks of the real desktop window, real notifications, measurement accuracy and
resource use are in [tests/desktop-e2e](tests/desktop-e2e/README.md). They need a desktop session
and open a visible window (one of them shows a real notification), so they are not part of
`cargo test`.

## CLI usage

```text
sentinel start            Start the daemon (reports the existing one if already running)
sentinel stop             Ask the daemon to shut down gracefully
sentinel status [--json]  Version, daemon state, uptime, IPC state, operating mode, config path
sentinel ui               Launch the desktop application
sentinel logs [-f] [-n N] [--json]   Show Sentinel's own log
sentinel scan [--hardware|--utilities] [--no-wait] [--json]   Scan (read-only)
sentinel scan --status | --cancel     Progress of, or stop, a scan
sentinel hardware [--json]            Show the hardware inventory
sentinel hardware capabilities        Which Linux information sources are readable
sentinel utilities [--all] [--category C] [--search S] [--json]
sentinel utilities services [--all]   Read-only list of systemd services
sentinel utilities open <id>          Open an installed application (desktop entries only)
sentinel monitor start|stop           Turn monitoring on or off (the choice is remembered)
sentinel monitor pause|resume         Pause sampling without turning monitoring off
sentinel monitor status [--json]      State, sampling profile, every collector's health
sentinel monitor profile eco|balanced|performance [--power-aware on|off]
sentinel metrics [cpu|memory|processes|battery|thermal|gpu|storage|network] [--json]
sentinel docs [--all] [--json]          # documentation results for this board
sentinel docs search [--refresh]        # search (library only unless online discovery is on)
sentinel docs download <id> | remove <id> | cancel | clear
sentinel alerts [--history] [--json]  Active alerts, or recent and past ones
sentinel alerts ack <id>              Mark an alert as seen (changes nothing on the system)
sentinel --help | --version
```

Exit codes: `0` success, `1` general error, `2` invalid arguments, `3` daemon not running,
`4` IPC failure, `5` invalid configuration, `6` permission denied, `7` unsupported environment,
`8` launch failure, `9` timeout, `11` request refused by the daemon, `12` scan cancelled,
`13` monitoring is off (from `sentinel metrics`).
`sentinel status` exits `3` when the daemon is stopped, like `systemctl status`.

## Where Sentinel keeps its files

| Purpose | Location |
|---|---|
| Configuration | `$XDG_CONFIG_HOME/sentinel/config.toml` (default `~/.config/sentinel`) |
| Database (session history, scan cache) | `$XDG_DATA_HOME/sentinel/sentinel.db` (default `~/.local/share/sentinel`) |
| Telemetry and alert history | `$XDG_DATA_HOME/sentinel/history.db` (24 h of 30 s detail, 30 days hourly, 90 days of alerts by default) |
| Logs | `$XDG_STATE_HOME/sentinel/logs/` (default `~/.local/state/sentinel`) |
| Cache | `$XDG_CACHE_HOME/sentinel` (default `~/.cache/sentinel`; unused so far) |
| Socket and lock | `$XDG_RUNTIME_DIR/sentinel/` |

Everything is created private to your user (`0700` directories, `0600` files) and stays on this
computer.

## Architecture overview

```text
 Desktop app (Tauri + React)        CLI (sentinel)
            \                          /
             +---- sentinel-client ---+      one shared backend interface
                         |
                   sentinel-ipc              versioned, typed protocol
                         |  Unix socket, this user only
                  sentinel-daemon            sentineld: event-driven main thread, a scan
                         |                   worker while scanning, a monitor thread while
                         |                   monitoring is on
     sentinel-hardware  sentinel-utilities   read-only discovery
     sentinel-telemetry                      read-only collectors, scheduler, metric model
     sentinel-alerts                         alert rules, health indicators, notifications
                         |
                  sentinel-inventory         typed model with provenance
                         |
     sentinel-config  sentinel-logging  sentinel-storage  sentinel-history
                         |
                   sentinel-core             errors, paths, state machines
```

Details: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Security model:
[docs/SECURITY.md](docs/SECURITY.md). Contributing and conventions:
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md). Discovery: [docs/HARDWARE_DISCOVERY.md](docs/HARDWARE_DISCOVERY.md),
[docs/SYSTEM_INVENTORY.md](docs/SYSTEM_INVENTORY.md), [docs/UTILITY_DISCOVERY.md](docs/UTILITY_DISCOVERY.md).
Telemetry: [docs/TELEMETRY_ENGINE.md](docs/TELEMETRY_ENGINE.md), [docs/METRIC_REFERENCE.md](docs/METRIC_REFERENCE.md),
[docs/ALERT_ENGINE.md](docs/ALERT_ENGINE.md), [docs/POWER_EFFICIENCY.md](docs/POWER_EFFICIENCY.md),
[docs/MONITORING_LIMITATIONS.md](docs/MONITORING_LIMITATIONS.md).
Reports: [docs/PHASE_1.md](docs/PHASE_1.md), [docs/PHASE_2.md](docs/PHASE_2.md), [docs/PHASE_3.md](docs/PHASE_3.md).
