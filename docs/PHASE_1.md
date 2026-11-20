# Phase 1 engineering report

Status: **implementation complete; one item unverified.** Everything except the Tauri desktop
shell has been built, run and tested on the development machine. The shell is written but could
not be compiled there, because the WebKitGTK development packages are not installed and
installing them needs administrator rights. Until it has been built and launched, Phase 1's
definition of done is not fully met.

## 1. Technical validation (Step 1)

| Area | Finding |
|---|---|
| System | Ubuntu 26.04.1 LTS, Linux 7.0, x86_64, GNOME on Wayland |
| Rust | stable 1.98.1 with Cargo, clippy, rustfmt |
| Node | 22.22.1, npm 9.2.0 |
| C toolchain | gcc 15.2, pkg-config, build-essential, libssl-dev |
| Unix sockets | Supported; `XDG_RUNTIME_DIR=/run/user/1000` |
| Graphics | Integrated Radeon 610M only; nothing assumes a dedicated GPU |
| Memory | 6.5 GiB; builds were run with four jobs |
| SQLite | No system development package; `rusqlite` compiles SQLite in |
| Tauri 2 prerequisites | Runtime libraries present; **development packages missing** |

Dependency decisions:

- Tauri is pinned to the 2.x line (resolved to 2.11.6). The newest release on crates.io is a 3.0
  alpha and was deliberately not used.
- `tracing-appender` was not used. It adds a writer thread; the daemon logs a few lines per
  session, so it writes synchronously and stays single-threaded.
- No `libc`/`nix` and no `unsafe`: file locking uses the standard library (`File::try_lock`,
  stable since Rust 1.89, hence the minimum Rust version) and `rustix` provides the user id.
- TOML for configuration; JSON lines for the IPC protocol and the log.

## 2. What was implemented

- Cargo workspace with seven library/daemon crates, the CLI, the desktop shell and a test package.
- `sentineld`: single instance, private Unix socket, typed IPC server, status, configuration,
  graceful shutdown on request or signal, stale-socket handling, unclean-shutdown detection,
  structured logging, session history in SQLite.
- `sentinel`: `start`, `stop`, `status [--json]`, `ui`, `logs [-f] [-n] [--json]`, with distinct
  exit codes.
- Versioned IPC protocol with all eight required requests plus `Shutdown`.
- Configuration with validation, migration, corruption recovery and atomic saves.
- React/TypeScript/Vite/Tailwind dashboard in the approved dark style: header with service
  switch and window controls, ten-item sidebar, system overview, hardware summary cards,
  motherboard panel, component information, telemetry frames, power mode and monitoring status,
  a working Settings page and labelled placeholders for the seven unbuilt sections.
- Tauri 2 shell exposing six commands over the shared client library (unverified, see section 8).
- Optional, manual systemd user unit.
- Documentation: README, ARCHITECTURE, SAFETY_POLICY, SECURITY, DEVELOPMENT and this report.

Not implemented, by design: hardware discovery, telemetry, the monitoring engine, diagnostics,
schematic search, notifications. The UI and CLI say so wherever these would appear.

## 3. Repository structure

```text
sentinel/
├── apps/
│   ├── cli/                      sentinel (main.rs, commands.rs, logs.rs)
│   └── desktop/
│       ├── src/                  React frontend (app, components, layouts, pages, hooks,
│       │                         services, styles, types)
│       └── src-tauri/            sentinel-desktop (main.rs, commands.rs, config, capabilities)
├── crates/
│   ├── sentinel-core/            errors, paths, instance lock, platform guards, state machines
│   ├── sentinel-config/          configuration model and store
│   ├── sentinel-logging/         structured file logging
│   ├── sentinel-storage/         SQLite schema and session history
│   ├── sentinel-ipc/             protocol, codec, blocking client
│   ├── sentinel-daemon/          sentineld
│   └── sentinel-client/          shared backend interface and launcher
├── resources/icons, resources/systemd
├── tests/                        integration/, safety/, support/, fixtures/
├── docs/
└── Cargo.toml, Cargo.lock, package.json, package-lock.json, README.md, .gitignore
```

Differences from the structure in the brief: `sentinel-client` was added (see section 4);
`tests/unit` was not created because Rust unit tests live beside the code they test; a
`tests/safety` suite was added.

## 4. Important architectural decisions

1. **One backend interface (`sentinel-client`).** The CLI and desktop call the same functions,
   so they cannot behave differently. This satisfies "do not implement two backends" by
   construction rather than by discipline.
2. **`flock` instead of a PID file.** The kernel releases the lock when the daemon dies, so
   Sentinel can never be stuck "already running", and PID reuse cannot confuse it.
3. **`Shutdown` as an IPC request.** Stopping the daemon never involves sending a signal, which
   keeps "Sentinel never kills processes" true without exception.
4. **Single-threaded, event-driven daemon.** No timers and no polling: measured zero wake-ups
   while idle.
5. **Monitoring as a seam, not a stub.** `MonitoringEngine` is a trait with a `NoEngine`
   implementation that refuses. The protocol, state machine and UI are final in shape; only the
   engine is missing, and nothing pretends otherwise.
6. **The header switch controls the service, not "monitoring".** The global ON/OFF control
   starts and stops the daemon, which is real. Monitoring has its own indicator that reads
   "Not available".
7. **Blocking IPC client.** Clients make short local requests; they need no async runtime.
8. **Preserve, never delete.** Corrupt configuration is moved aside, an unreadable database is
   left in place, a non-socket file at the socket path is refused, a newer configuration or
   database is not rewritten.
9. **Slow UI refresh.** Every five seconds while visible, immediately after actions, never while
   hidden. "Running since" is shown as a clock time so no per-second timer is needed.

## 5. Commands

```bash
npm install && cargo build            # set up and build daemon + CLI
cargo test && npm test                # all tests
./target/debug/sentinel start | status | stop | logs
npm run desktop:dev                   # desktop app (needs the WebKitGTK packages)
npm run desktop:build                 # release build and .deb
```

## 6. Test results

Run on the development machine after the final code change:

| Suite | Tests | Result |
|---|---|---|
| Rust unit tests (8 packages) | 116 | pass |
| Integration (real binaries) | 18 | pass |
| Safety (real binaries and source scans) | 14 | pass |
| Frontend (Vitest) | 14 | pass |
| `cargo fmt --check`, `cargo clippy --all-targets -D warnings` | | clean |
| `tsc --noEmit`, `vite build` | | clean |

The integration and safety suites were run eight times in a row without a failure.

Problems found by the tests during development and fixed:

- Under six simultaneous `sentinel start` commands exactly one daemon was created, but the
  others also printed "started". The client now compares the answering daemon's PID with the one
  it spawned and reports "already running".
- Two test-only timing races (a socket inspected while a client connection was closing; a
  process entry checked the instant the instance lock was released) were corrected in the tests.

Coverage gaps: the Tauri command layer has no automated test (it cannot be compiled here); the
`useSentinel` hook and click interactions are not tested in a DOM; `sentinel logs --follow` and
a successful `sentinel ui` launch are untested; the systemd unit passed `systemd-analyze verify`
but was never installed or started.

## 7. Resource consumption

Release build, daemon idle in a sandboxed home, measured from `/proc`:

| Measure | Value |
|---|---|
| `sentineld` binary | 2.33 MiB |
| `sentinel` binary | 1.09 MiB |
| Start to ready | 40 ms |
| Resident memory (RSS) | 4.9 MB, of which 0.48 MB private anonymous |
| Proportional memory (PSS) | 2.5 MB |
| Threads | 1 |
| Open file descriptors | 13 |
| CPU time over 60 s idle | 0 ticks |
| Context switches over 60 s idle | 0 |
| `sentinel status` (process start + round trip) | 7.7 ms average over 200 runs |
| RSS after 200 requests | unchanged (4.9 MB) |

The desktop application's resource use has not been measured.

## 8. Security and safety verification

Verified by the safety suite against the running daemon: it holds only Unix sockets; it runs
with the invoking user's ids and no capabilities; it has one thread and no child processes; it
does not wake while idle; a bystander process survives full start/stop cycles; the only files
created in a sandboxed home are the configuration, database, log and lock, all private.

Verified by source scans: processes are started only by the launcher and only for Sentinel's own
binaries; no package manager, service manager, privilege-escalation tool, shell, signal call,
network type or system path appears in production code; no `unsafe`; the IPC request set is
pinned; the webview has four window permissions and no plugins; the systemd unit is manual and
unprivileged. The frontend is checked to make no network requests and run no continuous
animation.

Not verified at runtime: the desktop shell's capability and CSP configuration (checked
statically and against the Tauri configuration schema only).

## 9. Definition of done

| Criterion | State |
|---|---|
| The project builds | Daemon, CLI, libraries, frontend: yes. Desktop shell: **not yet** |
| The desktop application launches | **Not verified** |
| The daemon runs independently | Yes |
| The CLI starts and stops the daemon | Yes |
| The desktop application communicates with the daemon | The shared client it uses is tested end to end; the path through the Tauri shell is **not verified** |
| The UI displays real application state | Implemented; verified by render tests and in a browser preview, **not yet in the shell** |
| The approved visual design is implemented | Built from the written specification (see limitation 1) |
| Configuration is persistent and validated | Yes |
| Logs are generated locally | Yes |
| Works without a dedicated GPU | Daemon and CLI: yes. Desktop: not verified |
| The daemon does not require root | Yes; it refuses to run as root |
| The daemon is lightweight while idle | Yes (section 7) |
| Tests pass | Yes |
| Safety boundaries verified | Yes (section 8) |
| Documentation complete | Yes |

## 10. Known limitations

1. **The reference image was not available.** The brief refers to an approved dashboard image
   "from the conversation". It was not present, so the dashboard follows the written design
   language and layout in section 9 of the brief. It should be compared with the image.
2. **The desktop shell is uncompiled.** Its Rust code was checked against the Tauri 2.11.6
   sources (`spawn_blocking`, command macros, permission identifiers) and its configuration
   validates against the Tauri schema, but compile errors remain possible.
3. Custom window controls on an undecorated window have not been tried on GNOME/Wayland.
4. The log rotates only at daemon start-up.
5. Configuration changes made by editing the file while the daemon runs are not picked up until
   it restarts.
6. The `.deb` bundle configuration (including shipping `sentineld` and `sentinel` in
   `/usr/bin`) is untested.
7. No licence has been chosen for the project.

## 11. Outstanding issues

To finish Phase 1:

1. Install the Tauri prerequisites:

   ```bash
   sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
     libjavascriptcoregtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev
   ```

2. Build and launch the desktop app (`npm run desktop:dev`), fix anything the compiler reports,
   and confirm in the window: the service switch starts and stops the daemon, the overview shows
   the daemon's real pid and start time, settings persist, window controls work, and closing the
   window leaves the daemon running.
3. Measure the desktop application's idle resource use and add it to section 7.
4. Compare the dashboard with the approved reference image.

Phase 2 has not been started.
