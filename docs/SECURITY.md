# Sentinel security and privacy

## Scope

Sentinel is a local application. It has no user accounts, no cloud backend, no telemetry upload,
no analytics, no remote control and no network listener. The frontend makes no network request.
Since Phase 6 the daemon can search public sources for motherboard documentation, **only if you
turn on online discovery** (off by default) and only when you start a search; automatic
downloads are a separate setting, also off. All of that network code is one module
(`crates/sentinel-docs/src/net.rs`), confined by the safety suite; see
[DOCUMENT_DISCOVERY.md](DOCUMENT_DISCOVERY.md) for its protections.

## Trust boundaries

```text
 other users / other machines        no access: nothing listens on the network
 ------------------------------------------------------------------------------
 the logged-in user
   webview (untrusted-ish UI code)   may call 29 fixed commands + 4 window controls
   desktop shell, CLI                same user; talk to the daemon over the socket
   daemon                            same user; validates every request
```

The daemon runs as the logged-in user and has exactly that user's permissions. It gives a client
nothing the client could not already do as that user. The validation below exists to keep the
daemon robust and to keep the protocol from growing into something dangerous later.

## IPC socket

- Location: `$XDG_RUNTIME_DIR/sentinel/daemon.sock`. The directory is `0700`, the socket `0600`.
- If `XDG_RUNTIME_DIR` is unset, the socket falls back to `~/.local/state/sentinel/run/`, with
  the same permissions.
- Every connection's peer is checked with `SO_PEERCRED`; a different user is disconnected.
- Messages are size-limited (64 KiB), version-checked and parsed into a closed set of typed
  requests. Unknown requests and unknown configuration fields are rejected.
- No request carries a command, a program path, a file path or free text; extra fields are
  rejected. The only open-ended-looking payload, a telemetry history query, is a list of at most
  32 series keys restricted to `[A-Za-z0-9._:/@-]` (at most 96 characters each) plus an
  enumerated time window; keys select series, they are never used as paths or SQL.
- Connections are limited in number (32) and idle time (30 s).

## Files

All of Sentinel's directories are created `0700` and verified to be real directories owned by
the current user; a symlink or a directory owned by someone else is refused. Configuration,
database, log and lock files are created `0600`. Existing Sentinel files with looser permissions
are tightened.

## Discovery

- Discovery reads only. The discovery crates contain no write API (enforced by a safety test), all
  hardware reads go through `SysRoot`, and a safety test checks every path a real scan touches
  against a short allow-list (`/sys`, parts of `/proc`, os-release, the ID databases).
- Identifiers of one machine or device (serial numbers, UUIDs, asset tags, PPIDs, WWNs) are never
  opened. MAC addresses are collected as the plan requires, kept only in the local inventory,
  masked in the UI until revealed, and never logged.
- The only processes discovery starts are four fixed, read-only `systemctl` listings. Their
  argument vectors are pinned by a safety test; no argument comes from data.
- Discovered programs are never executed; existence is checked with `stat`.
- Opening a utility is a user action handled by the client (never the daemon): the ID is validated,
  looked up in the discovered catalogue, and the desktop file is handed to `gio launch`. A desktop
  file's `Exec` line is never interpreted by Sentinel.
- The inventory and catalogue are cached in the private database (`0600`). They describe the
  machine and contain MAC addresses; they never leave it.

## Telemetry (Phase 3)

- Collectors only read, through the same `SysRoot` reader as discovery, so its refuse list
  applies: `environ`, process memory (`mem`, `maps`, `pagemap`...) and identifier files can never
  be opened. The telemetry crate contains no write API (safety test).
- Per-process data is limited to what `top` shows: pid, name (`comm`), owner, state, threads,
  CPU time, resident and virtual memory. The command line is read at most 256 bytes and cut at
  the first argument, so only the program name is kept; arguments, which often contain secrets,
  are never stored or shown. Environment variables are never read.
- Process data lives only in memory. It is never written to `history.db`.
- No packets are captured: network figures are the kernel's per-interface counters.
- History (`history.db`, `0600`) holds numeric series and alerts only, stays on this computer
  and is pruned by retention.
- The process list shows other users' processes the way `ps` does (Linux shows them to every
  user). Their details that the kernel protects stay unread and are counted as "not readable".

## Notifications

Alert notifications use the session D-Bus: Sentinel connects to the user's own session bus,
authenticates with `EXTERNAL` (the kernel-verified user id), and calls exactly one method,
`org.freedesktop.Notifications.Notify`. The client is a small hand-written module with no other
callable method. Notifications carry no actions, so clicking one cannot run anything. Markup in
alert text is escaped and control characters are removed. Delivery failures are recorded and
monitoring continues.

## Logs

Logs are local JSON-lines files. They record lifecycle events, error categories and Sentinel's
own paths. Sentinel does not log file contents, credentials, serial numbers, UUIDs or other
machine identifiers. `sentinel logs` can read only the daemon's log file; it has no option to
read another path or the system journal.

Note: Sentinel's own paths contain the user's home directory name. Logs stay on the machine.

## Desktop application

- The webview is granted four permissions: minimise, maximise, close and drag its own window.
- Its only free-form command argument is a utility ID, validated before use.
- No Tauri plugins are used: no shell, file-system, HTTP, clipboard, updater or process access.
- The content security policy allows only the app's own files.
- Errors shown in the UI are user-facing messages; detail goes to `desktop.log`.

## Privileges

Nothing in Sentinel runs as root, and the daemon and desktop refuse to start as root. There is no
privileged helper. The optional systemd unit is a user unit with `NoNewPrivileges=yes`,
`ProtectSystem=strict` and `RestrictAddressFamilies=AF_UNIX`.

## Document discovery (Phase 6)

Searches use the manufacturer, model, board identifier and revision only, in sanitised queries.
Serial numbers, UUIDs, SKUs, BIOS versions and the inventory are never sent or logged. Remote
documents are untrusted input: HTTPS only, public addresses only (checked again at connection
time against DNS rebinding), redirects re-validated, robots.txt and rate limits honoured, size
capped, content-sniffed, executables and web pages rejected, archives never extracted and
refused if any entry could escape a folder, files stored under their hash and never overwritten,
never opened or run automatically. The webview names documents by numeric ID; addresses and
paths come from the daemon's records and are re-validated before `gio open` is used.

## Known limitations

- Any process running as the same user can talk to the daemon. This is inherent to a per-user
  daemon and gives such a process nothing it could not already do.
- The log file is rotated only at daemon start-up (at 5 MiB, keeping one previous file).
- The desktop shell's capability and CSP configuration is verified statically by the safety
  tests; at runtime it was only observed working (see `PHASE_3.md`), not attacked.
- Any process of the same user can read the cached inventory, as with any of the user's files.
- A downloaded PDF is opened by the user's PDF viewer, which processes untrusted content; that
  viewer's security is outside Sentinel. Sentinel only opens it when you ask.

## Reporting a problem

Open an issue in the project's tracker describing what you observed. Do not include serial
numbers or other identifying hardware details.
