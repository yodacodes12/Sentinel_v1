# Power efficiency

How Sentinel keeps its own cost low, and what it actually costs on the development machine.

## Design

- **Off means off.** With monitoring off the daemon has one thread parked in `epoll_wait`: no
  timers, no polling, no reads. Monitoring adds one thread, which exists only while monitoring
  is on or paused.
- **One clock, aligned wake-ups.** The scheduler puts every collector on a fixed grid, and
  collectors due within 0.25 s run together, so the thread wakes about once per fastest interval
  rather than once per collector.
- **No fixed tick.** Between samples the thread sleeps until the next due time; paused, it sleeps
  with no timeout at all.
- **The expensive collector runs on demand.** The process list (three files per process) runs
  at its profile interval only while someone is looking at process data, otherwise once a minute.
- **Unsupported and failing sources back off** (5-minute re-check; exponential backoff up to
  5 minutes).
- **Batched history.** Samples are averaged into 30-second buckets in memory and written in one
  transaction every 5 minutes.
- **Power-aware sampling** (optional): Eco while on battery.
- **The desktop window polls only while its page is visible**, at the CPU sampling interval (at
  least 1 s), and never overlaps requests. Closing the window leaves the daemon exactly as it was.
  **On GNOME Wayland a minimised window still counts as visible** (the compositor does not tell
  applications they are minimised), so it keeps polling; measured below. Close the window to stop
  it.
- **No dedicated GPU needed**, and nothing is drawn by the daemon. The desktop charts are static
  SVG with no animation.

## Phase 4 measurements (final build)

Measured on 2026-11-19 with `tests/desktop-e2e/resources.py`, on the same laptop, on mains power
(charging), with normal desktop use in the background and a second Sentinel daemon running the
long-run session (below). Two runs: 120-s and 90-s windows per state. Release builds; real `/proc`
and `/sys`. Wake-ups are context switches summed over threads (an upper bound on wake-ups for the
daemon; for the window a lower bound, since threads that exit during a window are not counted).

### The daemon alone

| Mode | CPU (% of one core), run 1 / run 2 | Wake-ups/s | RSS | Threads | Read syscalls/s |
|---|---|---|---|---|---|
| Monitoring off | 0.000 / 0.000 | 0.0 / 0.0 | 5.8 / 6.1 MiB | 1 | 0 |
| Eco | 0.142 / 0.144 | 1.02 / 1.02 | 7.0 MiB | 2 | 136 / 108 |
| Balanced | 0.300 / 0.278 | 2.24 / 2.32 | 7.3 / 7.2 MiB | 2 | 196 / 230 |
| Performance | 0.575 / 0.556 | 5.05 / 5.06 | 7.4 / 7.3 MiB | 2 | 317 / 287 |

Consistent with Phase 3 below. These windows are shorter than the 5-minute history flush, so
most contain no database write; Phase 3's 320-s windows each contain one, which is part of why
its figures are higher. All wake-ups are the monitor thread's; with monitoring off there are none.

### The desktop window

| Window state | CPU (% of one core), run 1 / run 2 | Wake-ups/s | Memory (3 processes) | Threads |
|---|---|---|---|---|
| Overview, monitoring on (Balanced) | 1.93 / 3.01 | ≥ 131 / ≥ 262 | 437 / 453 MiB | 104 / 103 |
| Telemetry → CPU (charts), monitoring on | 2.11 / 4.06 | ≥ 186 / ≥ 449 | 441 / 456 MiB | 103 |
| Overview, monitoring off | 0.72 / 0.76 | n/a / ≥ 56 | 439 / 457 MiB | 100 / 92 |
| **Minimised**, monitoring on | 1.37 / 1.91 | ≥ 104 / ≥ 95 | 440 / 479 MiB | 102 / 93 |

(Run 1's wake-up count for "monitoring off" is not usable: threads that exited during the window
made the per-name sum negative; run 2 counts per thread.)

**Where the window's cost comes from.** Per thread (run 2, Overview, monitoring on): JavaScriptCore
garbage-collector helpers (`HeapHelper`, 119/s), the web process's main thread (29/s), the
compositor's frame scheduling (`VBlankMonitor`, 21/s), WebKit's inter-process queues
(`ReceiveQueue`, 17–19/s), the Tauri side answering commands (`tokio-rt-worker`, 13/s) and Skia's
GPU worker (9/s). The data is small: 17 KB of JSON per live poll, 9–35 KB per chart refresh. So
the cost is WebKit re-rendering and collecting garbage after each update, not the amount of data
or the daemon: it falls to 0.7 % with monitoring off (one status check every 5 s) and is highest
on the page with the most charts. Variation between the two runs is large (background desktop
activity, and more chart history to draw in run 2). The daemon's own cost of answering an open
window is small: the long-run daemon, answering three requests every 2 s, used about 0.33 % of a
core in Balanced against 0.28–0.30 % without a client.

Not optimised in Phase 4 (see PHASE_4.md, P4-15 and P4-16). For the lowest overhead, close the
window (the daemon keeps monitoring and notifying) or turn monitoring off.

### Database writes

History is written every 5 minutes (about 470 rows of 30-second buckets for about 47 series in
Balanced on this laptop). In the long run the bytes the daemon passed to `write()` per flush grew
with the database, from 69 KiB at the first flush to about 800 KiB when the file was 630 KiB;
between flushes it writes under 1 KiB a minute (its log). Reproduced on disk (ext4, NVMe) with
the same schema, 12 hours of synthetic data:

| Storage settings | Written per hour |
|---|---|
| **Current**: rollback journal, flush every 5 minutes | **11.6 MiB** (about 280 MiB a day while monitoring) |
| WAL journal with `synchronous=NORMAL`, every 5 minutes | 7.1 MiB |
| Rollback journal, flush every 15 minutes | 4.7 MiB |
| WAL and every 15 minutes | 3.2 MiB |

Why: rows are stored in `(series, time)` order, so every flush adds a few rows to the end of
each series' range, touching at least one 4-KiB page per series (and splitting pages as they
fill), and the rollback journal writes each touched page twice. The data itself is about 20 KB
per flush. This is about 0.1 TB a year: negligible for SSD endurance (consumer SSDs are
typically rated for hundreds of TB written), and each flush takes milliseconds, but it is real write
amplification. It is **not changed in Phase 4**, because each remedy has a cost the user should
choose: WAL adds two files beside the database and changes how the CLI reads history while the
daemon is stopped; a longer flush interval means a crash loses up to 15 minutes of history
instead of 5; a time-ordered layout needs a schema migration. Recorded as P4-24 in
[PHASE_4.md](PHASE_4.md). Growth is bounded by retention: about 330 KiB an hour, so roughly 8 MB
at the default 24 hours of detail, plus the hourly roll-ups.

## Long run (2.5 hours)

`tests/desktop-e2e/longrun.py`, 2026-11-19 12:05–14:35, release daemon on the real `/proc` and
`/sys`, power-aware sampling on, no session bus (notifications fail and are recorded). A client
imitating an open window made three requests (status, metrics, alerts) on fresh connections
every 2 s. One sample per minute; the home directory was on `tmpfs`.

| | Result |
|---|---|
| Duration | 9001 s (150 one-minute samples); monitoring `active` throughout |
| Profile | Eco for the first 12 minutes (on battery), Balanced for 138 (charger connected): switched by itself |
| CPU (one core) | Eco 0.32 % mean; Balanced 0.32 % mean, 0.38 % worst minute (includes answering the client) |
| Wake-ups | Eco 4.7/s, Balanced 5.9/s mean (10.6 worst minute), including the client's requests |
| Memory (RSS) | 7.1 MiB at start, 5.3–7.9 MiB range, 7.7 MiB at the end; quarter means 6.8 / 6.6 / 6.8 / 7.4 MiB (no growth trend); virtual size 74.7 → 74.8 MiB |
| Threads, descriptors | 2 and 14 in every sample |
| Client | 13 488 requests, 0 errors, mean 0.45 ms, worst 13.1 ms |
| History | 13 677 rows, 880 KiB file after 2.5 h (337 KiB an hour); writes in one burst per flush, growing with the file (see "Database writes") |
| Alerts | One real `battery_low` alert at 15 % while discharging, resolved when the charger was connected; its notification failed (no bus) and was recorded |
| Clock | Wall and monotonic time never diverged more than 0.1 s: no suspend happened, so resume handling was not exercised |
| Log | 2.2 KB, one warning (the notification failure), no errors |


## Phase 3 measurements

Measured on 2026-11-18 on the development laptop: Dell Inspiron 15 3535, AMD Ryzen 3 7320U
(4 cores, 8 threads), 6.5 GiB RAM, Radeon 610M integrated GPU, NVMe SSD, Ubuntu 26.04, kernel 7.0,
GNOME on Wayland. Release builds. Normal desktop use in the background (a browser, a terminal).
The daemon ran in a throwaway home directory against the real `/proc` and `/sys`.

Method (`bench.py`, kept outside the repository): for each mode, the daemon's
`/proc/PID/stat` CPU time, the context switches of **all** its threads, `/proc/PID/io` and
`VmRSS` were read before and after a fixed window. Nothing talked to the daemon during a window.
Each monitoring window was 320 s, so it includes one history flush. "Wake-ups" are context
switches per second, which is an upper bound on wake-ups.

| Mode | CPU (% of one core) | Wake-ups/s | RSS | Threads | Read syscalls/s | Written to files | History rows |
|---|---|---|---|---|---|---|---|
| Daemon running, monitoring off (120 s) | **0.000** | **0.0** | 4.4 MiB | 1 | 0 | 0 | 0 |
| Monitoring on, Eco | 0.21 | 1.05 | 7.0 MiB | 2 | 126 | 201 KiB / 320 s | 470 |
| Monitoring on, Balanced | 0.48 | 2.38 | 7.3 MiB | 2 | 199 | 233 KiB / 320 s | 470 |
| Monitoring on, Performance | 0.94 | 5.18 | 8.1 MiB | 2 | 302 | 281 KiB / 320 s | 470 |
| Desktop window open on the Overview, Balanced (180 s): daemon | 0.54 | 4.87 | 7.8 MiB | 2 | 190 | 2 KiB | — |
| … the desktop app itself (Tauri shell + 2 WebKit processes, 120 s) | 1.81 | 98 | 422 MiB | 99 | 264 | 127 KiB | — |
| Desktop window closed, monitoring on, Balanced (180 s, after 70 s settling) | 0.47 | 2.24 | 8.5 MiB | 2 | 188 | 0 | — |

With the window open, the daemon's own cost rises a little (it answers the window's requests
every 2 s). The desktop app costs several times more than the daemon in every monitoring mode:
WebKit's processes account for most of its memory and wake-ups. Closing the window returns the
daemon to its baseline. For the lowest overhead, monitor with the window closed and use
`sentinel metrics` or notifications.

"Written to files" is the bytes Sentinel passed to `write()` (history database and its journal,
the log). The number of history rows does not depend on the profile, because buckets are
30 seconds wide whatever the sampling interval. The kernel attributed no device writes to the
daemon in any window (`write_bytes` 0). **Correction (Phase 4):** this was not because the writes
stayed in the page cache, as first stated here: the throwaway home was under `/tmp`, which is a
`tmpfs` (memory) on this system, so nothing could reach a disk. On a real installation the
history is on disk and SQLite syncs at every commit; see "Database writes" in the Phase 4
measurements above. The 201–281 KiB per 320 s in the table were also measured early in a fresh
database; the volume per flush grows with the database (below).

### Time per collector run (Balanced, last run in each window)

| Collector | Time | Note |
|---|---|---|
| System load | 0.08 ms | |
| Memory | 0.5 ms | |
| CPU | 0.6 ms | 8 logical CPUs, plus cpufreq per CPU |
| Network | 0.6–1.1 ms | 3 interfaces (Wi-Fi, Docker bridge, loopback) |
| GPU | 1.2–1.9 ms | amdgpu sysfs and hwmon |
| Storage | 1.5–2.6 ms | diskstats, mounts, `statvfs` per local filesystem |
| Fans | 2.7–4.8 ms | one `dell_smm` fan: a firmware (SMM) call per read |
| Battery | 2.7–4.3 ms | |
| Temperatures | 14–17 ms (one outlier of 35 ms) | 9 sensors; `dell_smm` reads cost about 1.2 ms each and NVMe temperature reads about 3 ms each (a query to the drive) |
| Processes | 26–28 ms | about 330 processes, three files each |

The temperature collector is the most expensive regular collector on this machine, and the cost
is in the drivers, not in Sentinel: timing the same `read()`s from a separate script gave about
5 ms per pass for the two `dell_smm` and `nvme` chips alone. It runs every 10 s (Eco), 5 s
(Balanced) or 2 s (Performance).

### Battery

Not measured. The laptop was put on its charger part-way through the Eco window, and the
system's own power draw (4.5 W idle on battery, about 20 W while charging) varies far more than
Sentinel's share could. Sentinel uses well under 1 % of one core in every mode, but **no claim
about battery life is made** without a controlled measurement: several hours on battery with a
fixed workload, monitoring on and off, repeated.

### How to reproduce

```bash
T=$(mktemp -d) && mkdir -m 700 "$T/run"
export HOME="$T" XDG_RUNTIME_DIR="$T/run"
./target/release/sentinel start
./target/release/sentinel monitor profile eco --power-aware off
./target/release/sentinel monitor start
PID=$(./target/release/sentinel status --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["daemon"]["pid"])')
# CPU ticks (fields 14 and 15), then again after N seconds:
awk '{print $14 + $15}' /proc/$PID/stat
# wake-ups: sum over threads
cat /proc/$PID/task/*/status | grep ctxt_switches
```

## Limits of these numbers

- One machine, one session, one run per mode. Other hardware differs, above all in sensor drivers
  (see the temperature row).
- Background desktop activity was not controlled; it affects collector times more than
  Sentinel's CPU share.
- The process collector's cost grows with the number of processes.
