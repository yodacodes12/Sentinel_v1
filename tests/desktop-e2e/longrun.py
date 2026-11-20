#!/usr/bin/env python3
"""Long-run reliability session for the Sentinel daemon (Phase 4, workstream J).

Runs the release daemon against the real /proc and /sys, in a throwaway home
with no session bus (so no notification can appear on screen). A client
thread behaves like an open desktop window: every 2 s it opens a fresh
connection and asks for status, latest metrics and active alerts. Every 60 s
the daemon's resources are recorded as one JSON line.

Usage: longrun.py <bin_dir> <home> <rundir> <out.jsonl> <seconds>
"""
import json, os, socket, sqlite3, subprocess, sys, threading, time

bin_dir, home, rundir, out_path, total = sys.argv[1:6]
total = float(total)
CLI = f"{bin_dir}/sentinel"
ENV = {"HOME": home, "XDG_RUNTIME_DIR": rundir, "PATH": os.environ["PATH"],
       "SENTINEL_DAEMON_BIN": f"{bin_dir}/sentineld"}
SOCK = f"{rundir}/sentinel/daemon.sock"
DB = f"{home}/.local/share/sentinel/history.db"
TICKS = os.sysconf("SC_CLK_TCK")

stats = {"requests": 0, "errors": 0, "max_latency_ms": 0.0, "latency_sum_ms": 0.0}
lock = threading.Lock()
stop = threading.Event()


def cli(*args):
    return subprocess.run([CLI, *args], env=ENV, capture_output=True, text=True)


def ask(req_id, request):
    t0 = time.monotonic()
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(10)
    s.connect(SOCK)
    s.sendall(json.dumps({"version": 1, "id": req_id, "request": request}).encode() + b"\n")
    buf = b""
    while not buf.endswith(b"\n"):
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
    s.close()
    reply = json.loads(buf)
    return reply, (time.monotonic() - t0) * 1000


def window_client():
    n = 0
    while not stop.wait(2.0):
        for req in ({"type": "GetDaemonStatus"}, {"type": "GetLatestMetrics"},
                    {"type": "GetActiveAlerts"}):
            n += 1
            try:
                reply, ms = ask(n, req)
                ok = reply["outcome"]["status"] == "ok"
            except Exception:
                ok, ms = False, 0.0
            with lock:
                stats["requests"] += 1
                stats["errors"] += 0 if ok else 1
                stats["latency_sum_ms"] += ms
                stats["max_latency_ms"] = max(stats["max_latency_ms"], ms)


def sample(pid):
    rest = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
    status = dict(l.split(":", 1) for l in open(f"/proc/{pid}/status") if ":" in l)
    ctx = 0
    for task in os.listdir(f"/proc/{pid}/task"):
        ts = dict(l.split(":", 1) for l in open(f"/proc/{pid}/task/{task}/status") if ":" in l)
        ctx += int(ts["voluntary_ctxt_switches"]) + int(ts["nonvoluntary_ctxt_switches"])
    io = dict(l.split(":", 1) for l in open(f"/proc/{pid}/io"))
    files = {}
    for suffix in ("", "-wal", "-journal", "-shm"):
        p = DB + suffix
        files[os.path.basename(p)] = os.path.getsize(p) if os.path.exists(p) else None
    rows = alerts = None
    try:
        c = sqlite3.connect(f"file:{DB}?mode=ro", uri=True, timeout=1)
        rows = c.execute("SELECT COUNT(*) FROM samples").fetchone()[0]
        alerts = c.execute("SELECT COUNT(*) FROM alerts").fetchone()[0]
        c.close()
    except sqlite3.Error as e:
        rows = f"error: {e}"
    log = f"{home}/.local/state/sentinel/logs/daemon.log"
    return {
        "cpu_ticks": int(rest[11]) + int(rest[12]),
        "ctx": ctx,
        "rss_kib": int(status["VmRSS"].split()[0]),
        "vmsize_kib": int(status["VmSize"].split()[0]),
        "threads": int(status["Threads"]),
        "fds": len(os.listdir(f"/proc/{pid}/fd")),
        "wchar": int(io["wchar"]),
        "db_files": files,
        "sample_rows": rows,
        "alert_rows": alerts,
        "log_bytes": os.path.getsize(log) if os.path.exists(log) else None,
    }


cli("start")
cli("monitor", "profile", "balanced", "--power-aware", "on")
cli("monitor", "start")
pid = json.loads(cli("status", "--json").stdout)["daemon"]["pid"]
threading.Thread(target=window_client, daemon=True).start()

t_start_mono, t_start_wall = time.monotonic(), time.time()
prev, prev_t = sample(pid), time.monotonic()
out = open(out_path, "a")
while time.monotonic() - t_start_mono < total:
    time.sleep(60)
    try:
        cur = sample(pid)
    except (OSError, KeyError) as e:
        out.write(json.dumps({"t": time.time(), "fatal": f"daemon gone: {e}"}) + "\n")
        out.flush()
        break
    now = time.monotonic()
    dt = now - prev_t
    with lock:
        s = dict(stats)
    st = json.loads(cli("monitor", "status", "--json").stdout or "{}")
    rec = {
        "t_wall": round(time.time(), 1),
        "elapsed_mono_s": round(now - t_start_mono, 1),
        "elapsed_wall_s": round(time.time() - t_start_wall, 1),
        "cpu_pct": round((cur["cpu_ticks"] - prev["cpu_ticks"]) / TICKS / dt * 100, 3),
        "wakeups_per_s": round((cur["ctx"] - prev["ctx"]) / dt, 2),
        "written_kib": round((cur["wchar"] - prev["wchar"]) / 1024, 1),
        **{k: cur[k] for k in ("rss_kib", "vmsize_kib", "threads", "fds", "db_files",
                               "sample_rows", "alert_rows", "log_bytes")},
        "client_requests": s["requests"], "client_errors": s["errors"],
        "client_max_latency_ms": round(s["max_latency_ms"], 1),
        "client_mean_latency_ms": round(s["latency_sum_ms"] / max(s["requests"], 1), 2),
        "phase": st.get("phase"),
        "effective_profile": st.get("sampling", {}).get("effective"),
        "notify_failures": st.get("notifications", {}),
    }
    out.write(json.dumps(rec) + "\n")
    out.flush()
    prev, prev_t = cur, now
stop.set()
out.write(json.dumps({"done": True, "t": time.time()}) + "\n")
out.close()
