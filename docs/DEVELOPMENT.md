# Developing Sentinel

## Setup

See the README for prerequisites. Then:

```bash
npm install
cargo build
cargo test
npm test
```

Dependency versions are pinned by `Cargo.lock` and `package-lock.json`; commit both.

## Everyday commands

| Task | Command |
|---|---|
| Build daemon, CLI, libraries | `cargo build` |
| Run all Rust tests | `cargo test` |
| Run one suite | `cargo test -p sentinel-tests --test safety` (or `integration`, `hardware`) |
| Inventory of this machine as JSON | `cargo run -p sentinel-hardware --example scan` (optional argument: a fixture root) |
| Utility catalogue as JSON | `cargo run -p sentinel-utilities --example utilities` |
| Frontend tests | `npm test` |
| Frontend type-check and bundle | `npm run build` |
| Frontend in a browser | `npm run dev` (shows "preview mode": no daemon access) |
| Desktop app, live reload | `npm run desktop:dev` |
| Desktop release and `.deb` | `npm run desktop:build` |
| Format | `cargo fmt --all` |
| Lint | `cargo clippy --all-targets -- -D warnings` |

The desktop shell is a workspace member but not a default member, so plain `cargo build` and
`cargo test` do not need the WebKitGTK packages. Target it with `-p sentinel-desktop`.

## Trying it without touching your own data

Point Sentinel at a throwaway home:

```bash
T=$(mktemp -d) && mkdir -m 700 "$T/run"
env HOME="$T" XDG_RUNTIME_DIR="$T/run" ./target/debug/sentinel start
env HOME="$T" XDG_RUNTIME_DIR="$T/run" ./target/debug/sentinel status
env HOME="$T" XDG_RUNTIME_DIR="$T/run" ./target/debug/sentinel monitor start
env HOME="$T" XDG_RUNTIME_DIR="$T/run" ./target/debug/sentinel metrics
env HOME="$T" XDG_RUNTIME_DIR="$T/run" ./target/debug/sentinel stop
```

Keep `$T` short (`mktemp -d` in `/tmp` is fine): the socket path must stay under 107 bytes.
To watch monitoring work without depending on your machine's load, point the daemon at a
synthetic tree with `SENTINEL_DISCOVERY_ROOT` (see `tests/integration/telemetry.rs` for one that
keeps its counters moving).

Useful environment variables:

| Variable | Effect |
|---|---|
| `SENTINEL_LOG` | Log level: `error`, `warn`, `info` (default), `debug`, `trace` |
| `SENTINEL_DAEMON_BIN` | Path to `sentineld`, overriding the default lookup |
| `SENTINEL_DESKTOP_BIN` | Path to `sentinel-desktop`, overriding the default lookup |
| `SENTINEL_DISCOVERY_ROOT` | Daemon reads hardware and telemetry from this directory instead of `/` (tests; inventories are labelled) |

By default the binaries are looked up next to the running program, then on `PATH`.

## Conventions

- **Errors.** Library code returns `sentinel_core::Result`. Every `SentinelError` message is
  written for an ordinary user; technical detail goes in the `source` and the logs. Binaries map
  errors to exit codes with `SentinelError::exit_code`.
- **No panics in normal operation.** `unwrap` and `expect` are for tests and for invariants that
  cannot fail.
- **No `unsafe`.** It is forbidden workspace-wide.
- **State changes are explicit.** Lifecycle changes go through the state machines in
  `sentinel-core::state`; illegal transitions are errors.
- **Honesty.** A request is acknowledged only if what it asked for is true afterwards. The UI
  never shows a value that was not measured. Stored-only settings are labelled as such.
- **Small modules.** Request handling (`handler.rs`) is separate from socket I/O (`server.rs`) so
  it can be tested without a socket.
- **Frontend.** Only `services/bridge.ts` may import `@tauri-apps/*`. Display logic that carries
  an honesty rule lives in `app/viewModel.ts` as pure functions with tests. No continuous
  animation and no network requests; both are enforced by tests.

## Tests

- **Unit tests** sit beside the code in `#[cfg(test)]` modules.
- **Integration tests** (`tests/integration`) and **safety tests** (`tests/safety`) run the real
  `sentinel` and `sentineld` binaries. The harness (`tests/support`) builds them first and gives
  every test a `Sandbox`: a temporary directory used as `HOME` and `XDG_RUNTIME_DIR`, with the
  rest of the environment cleared.
- **Telemetry tests**: `crates/sentinel-telemetry/tests/fixture_collectors.rs` runs every
  collector through the real engine against a synthetic `/proc` and `/sys`, with expected values
  worked out by hand in the comments. `tests/integration/telemetry.rs` runs the daemon on a
  synthetic laptop whose counters a writer thread keeps advancing at a fixed ratio (so a CPU
  delta is always exactly 75 %), with a fake session bus for notifications. None of them load
  or depend on the machine running them; the plan forbids stress tests on the real machine
  without permission.
- **Hardware tests** (`tests/hardware`) run discovery against synthetic trees built by
  `tests/support/hwfixture.rs` (AMD and Intel hybrid laptops, a desktop with placeholder DMI and a
  restricted sensor, an ARM board) and, read-only, against this machine, checking results against
  independent readings rather than fixed values. Fixture data is invented; never copy a real
  machine's identifiers into a fixture.
- **Hardware explorer** (`tests/hardware/explorer.rs`): the component index of every fixture
  machine (hierarchies, cache sharing, storage layering, orphans, stable IDs) and, read-only, of
  this machine, where every live telemetry key must map to a component. The same test writes the
  frontend's fixtures `apps/desktop/src/test-fixtures/explorer-*.json` from the AMD and Intel
  fixture machines; after changing the inventory or the index, regenerate them with
  `SENTINEL_UPDATE_FIXTURES=1 cargo test -p sentinel-tests --test hardware` (the test fails while
  they are stale).
- **Document discovery** (`crates/sentinel-docs`): identity, queries, classification, matching,
  trust, robots.txt, address checks, downloads and providers against a scripted `Transport`;
  routine tests never touch the network. Two `#[ignore]` tests contact public sites
  (`cargo test -p sentinel-docs -- --ignored live`), using a generic model, never the machine's
  own identity. `engine.rs` writes the frontend fixture
  `apps/desktop/src/test-fixtures/documents-view.json` (regenerate with
  `SENTINEL_UPDATE_FIXTURES=1 cargo test -p sentinel-docs`). The daemon coordinator is tested in
  `crates/sentinel-daemon/src/documents.rs`, the binaries in `tests/integration/documents.rs`.
- **Alert rules** (`crates/sentinel-alerts/tests/rules.rs`): every rule driven through the real
  engine with synthetic snapshots (hold, severity, duplicates, band, clear time, cooldown).
- **Reliability tests** (`tests/integration/reliability.rs`): a daemon frozen with `SIGSTOP`, a
  crash during monitoring, connection churn, rapid control changes, invalid settings, machines
  without a battery or GPU. A test that freezes its daemon resumes it even when it fails, so no
  stopped process is left behind.
- **Frontend**: `npm test` (vitest, pure logic and static rendering). Behaviour that needs a
  running page (polling, typing into Settings) is checked in headless Firefox by
  `tests/desktop-e2e/browser_settings.mjs`.
- **Desktop end-to-end** (`tests/desktop-e2e`): the real window driven through AT-SPI, real
  notifications, accuracy against kernel counters, resource use, a long run. Manual: they need a
  desktop session and open a visible window. See its README.
- Tests never use the developer's real Sentinel directories and never perform destructive
  operations. The only processes a test terminates are ones that test itself started.
- The safety suite includes source scans. If you add a legitimate new capability and a scan
  fails, update the scan in the same change and explain why in review.

## Adding things

See "Extension points" in [ARCHITECTURE.md](ARCHITECTURE.md). In short:

- A new IPC request: add the variant in `sentinel-ipc/src/protocol.rs`, handle it in
  `sentinel-daemon/src/handler.rs`, add a client method, update the codec round-trip test and
  the pinned list in `tests/safety`.
- A new setting: add it to `SentinelConfig` and `ConfigPatch` with a default; add a migration
  step if existing files need changing.
- A new table: append a migration in `sentinel-storage`. Never edit an existing migration.
- A new document source: a `Provider` in `crates/sentinel-docs/src/providers`, using only
  `net::Client` for network access. Do not add an HTTP client anywhere else: the safety suite
  rejects it.
- A new discovery module: see "Adding a discovery module" in
  [HARDWARE_DISCOVERY.md](HARDWARE_DISCOVERY.md).
- A new collector or alert rule: see [TELEMETRY_ENGINE.md](TELEMETRY_ENGINE.md) and
  [ALERT_ENGINE.md](ALERT_ENGINE.md). A collector reads only through `SysRoot`, returns a
  `RunReport`, and must turn every problem into a reading state, never a number.

## Before you commit

```bash
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test && npm run build && npm test
```

Update the documentation in the same change. Do not document planned work as finished.
