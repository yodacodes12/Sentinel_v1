# Daemon and IPC reliability report (Phase 4)

How the daemon, the CLI and the desktop window behave when the other side is missing, slow,
broken or restarting. Environment: as in [DESKTOP_INTEGRATION.md](DESKTOP_INTEGRATION.md).

## Design, as verified

- **One daemon per user**, enforced by a lock file; a second `start` finds the first
  (`starting_twice_keeps_a_single_instance`, `concurrent_starts_produce_exactly_one_daemon`).
- **One request per connection** from the CLI and the window; no client keeps a connection open,
  so there is no connection state that can go stale. The long run made 13 488 such connections.
- **Bounded waiting everywhere.** Client read and write timeout 3 s; `start`/`stop` wait at most
  5 s; monitoring commands at most 5 s. The daemon closes connections idle for 30 s and serves
  at most 32 at once; extra ones are closed at once, not queued.
- **The window never blocks.** Every backend call runs on Tauri's blocking pool, never the UI
  thread. Phase 4 added: background polls never overlap (a slow answer delays the next poll
  instead of stacking requests), so at most one request per kind is in flight.
- **No reconnection loop.** The window polls the daemon's status every 5 s (live data at the
  sampling interval while monitoring is active, nothing while the window is hidden). With no
  daemon, a poll is a failed `connect()` on a missing socket: microseconds, no retries, no
  backoff needed. With a hung daemon, each poll waits its 3-s timeout and the next one waits for
  it.
- **The window shows what it knows.** Phase 4 made the three cases distinct: daemon not running
  ("OFF", monitoring "Inactive", no live data), daemon not answering ("NO REPLY", monitoring
  "Unknown", last data marked "Not live · Sentinel is not answering", the start switch disabled so
  no second daemon is attempted), and connected. A polling error disappears once the daemon
  answers again; an error from the user's own action stays until dismissed.

## Scenarios

| # | Scenario | How tested | Result |
|---|---|---|---|
| 1 | Start Sentinel normally | `cli_starts_queries_and_stops_the_daemon`; real window startup | Pass |
| 2 | Start the daemon independently (`sentineld`) | `sigterm_shuts_the_daemon_down_gracefully` and others spawn `sentineld` directly | Pass |
| 3 | Open the window with the daemon already running | Real window | Pass: live data 2.2 s after launch |
| 4 | Restart the window without restarting the daemon | Real window: close, relaunch | Pass: same daemon, still one monitor thread |
| 5 | Stop the daemon while the window is open | Real window | Pass: "OFF" 0.7 s later, no stale data, no banner |
| 6 | Restart the daemon while the window is open | Real window | Pass: live again within one refresh (4.9 s), monitoring resumed by preference |
| 7 | IPC connection failure | Stale socket (`stale_socket_is_replaced_on_start`), foreign file at the socket path (`foreign_file_at_the_socket_path_is_never_deleted`), crash (below) | Pass |
| 8 | Slow IPC response | Daemon frozen with `SIGSTOP`: `a_frozen_daemon_makes_requests_fail_in_bounded_time_and_then_recovers`; real window; Firefox with a 9-s bridge | Pass: client times out in 3 s; `sentinel status` says "Daemon: not answering" with exit code 4 (was "stopped"); window "NO REPLY" in 8 s and stays responsive; after `SIGCONT` the same daemon answers, still one monitor, monitoring still sampling |
| 9 | Invalid or incomplete response | Client unit tests: garbage, mismatched id, wrong variant, disconnect mid-answer, silent daemon | Pass: `protocol`/`ipc`/`timeout` errors, never a panic or a hang |
| 9b | Invalid or hostile request (server side) | `invalid_ipc_requests_are_rejected_without_harming_the_daemon`, server unit tests (malformed JSON, unknown type, oversized frame, half frames, idle connections, connection limit) | Pass |
| 10 | Repeated connection and disconnection | `connection_churn_leaks_no_descriptors_or_threads`: 300 mixed connections (normal requests, silent connects, half frames) plus 40 concurrent clients × 10 requests beyond the 32-connection limit; long run: 13 488 connections over 2.5 h | Pass: descriptors return to their level, no extra threads; long run: 0 failed requests (see below) |
| 11 | Recovery after unexpected termination | `daemon_restarts_after_unexpected_termination`; `monitoring_comes_back_once_after_a_crash`; real window (`SIGKILL`, start from the window) | Pass: stale socket replaced, `recovered_from_unclean_shutdown` reported and shown in the window, monitoring resumes once (by preference), every collector listed once |

Additional checks:

| Check | How tested | Result |
|---|---|---|
| No duplicate monitor after many control changes | `rapid_control_changes_never_duplicate_the_monitor`: 5 × (start, 3 profile changes, pause, refused start, resume, refused resume, refused start, stop) | Pass: 2 threads while monitoring, 1 when off, every time |
| A collector stuck in a read cannot lead to two monitors | `a_hung_collector_never_leads_to_a_second_monitor_thread` (a FIFO in place of `/proc/loadavg`) | Pass after the Phase 4 fix (before: `stop` gave up on the thread after 5 s and a later `start` ran a second monitor beside it) |
| Settings survive a daemon restart; invalid values change nothing | `invalid_settings_are_refused_and_change_nothing` | Pass: out-of-range percentage, critical above warning and zero retention refused with the reason; the file is byte-for-byte unchanged |
| Profile survives a daemon restart | `rapid_control_changes_never_duplicate_the_monitor` | Pass: Eco selected and in effect after the restart, CPU every 5 s |

## Long-run client statistics

See [POWER_EFFICIENCY.md](POWER_EFFICIENCY.md#long-run-25-hours) for the full long-run results.
A client imitating an open window made three requests on fresh connections every 2 s for the
whole 2.5-hour session: 13 488 requests, 0 errors, mean latency 0.45 ms, worst 13.1 ms; the
daemon's descriptors stayed at 14 and its threads at 2.

## Protocol changes in Phase 4

None to the framing, the request set or `PROTOCOL_VERSION` (still 1). One field changed meaning:
`RuntimeEstimate::Estimated.average_power_w` is now always watts and may be `null` (it carried
amperes for charge-reporting batteries). A client that assumed a number must accept `null`; the
desktop and the CLI are updated.

## Known limits

- The window learns that the daemon stopped answering only at its next status refresh plus the
  3-s timeout: up to about 8 s.
- A request to a hung daemon holds one blocking-pool thread for up to 3 s; polls do not overlap,
  so this is at most one thread per kind of request (observed: 38 → 39 threads).
- `sentinel status` reports a hung daemon with exit code 4 (IPC failure), not 9 (timeout): the
  overview does not keep the error kind.
