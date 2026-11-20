#!/usr/bin/env python3
"""Phase 4 resource measurements (final build), real /proc and /sys.

Part 1: the daemon alone, per mode. Part 2: the desktop window in several
states, with per-thread wake-ups to find where they come from. Nothing
talks to the daemon during a window except the desktop app under test.
Usage: resources.py <repo> <workdir> <seconds-per-mode>
"""
import collections, json, os, shutil, subprocess, sys, tempfile, time

from ui import UI

repo, work, secs = sys.argv[1], sys.argv[2], float(sys.argv[3])
B = f"{repo}/target/release"
HOME = f"{work}/home"
shutil.rmtree(HOME, ignore_errors=True)
os.makedirs(f"{HOME}/.config/sentinel")
open(f"{HOME}/.config/sentinel/config.toml", "w").write("version = 2\nnotifications_enabled = false\n")
RUN = tempfile.mkdtemp(prefix="snl-b.", dir="/tmp")
os.chmod(RUN, 0o700)
ENV = {"HOME": HOME, "XDG_RUNTIME_DIR": RUN, "PATH": os.environ["PATH"], "SENTINEL_DAEMON_BIN": f"{B}/sentineld"}
GENV = dict(ENV)
for k in ("DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP", "XDG_DATA_DIRS", "LANG"):
    if k in os.environ:
        GENV[k] = os.environ[k]
GENV["WAYLAND_DISPLAY"] = os.path.join("/run/user/%d" % os.getuid(), os.environ.get("WAYLAND_DISPLAY", "wayland-0"))
TICKS = os.sysconf("SC_CLK_TCK")
DB = f"{HOME}/.local/share/sentinel/history.db"


def cli(*a):
    return subprocess.run([f"{B}/sentinel", *a], env=ENV, capture_output=True, text=True, timeout=30)


def tree(pid):
    out, todo = [], [pid]
    while todo:
        p = todo.pop()
        out.append(p)
        try:
            for t in os.listdir(f"/proc/{p}/task"):
                todo += [int(c) for c in open(f"/proc/{p}/task/{t}/children").read().split()]
        except OSError:
            pass
    return out


def sample(pids):
    s = {"ticks": 0, "rss_kib": 0, "threads": 0, "wchar": 0, "syscr": 0, "per_thread": collections.Counter()}
    for pid in pids:
        try:
            comm = open(f"/proc/{pid}/comm").read().strip()
            rest = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
            s["ticks"] += int(rest[11]) + int(rest[12])
            st = dict(l.split(":", 1) for l in open(f"/proc/{pid}/status") if ":" in l)
            s["rss_kib"] += int(st["VmRSS"].split()[0])
            s["threads"] += int(st["Threads"])
            io = dict(l.split(":", 1) for l in open(f"/proc/{pid}/io"))
            s["wchar"] += int(io["wchar"])
            s["syscr"] += int(io["syscr"])
            for t in os.listdir(f"/proc/{pid}/task"):
                ts = dict(l.split(":", 1) for l in open(f"/proc/{pid}/task/{t}/status") if ":" in l)
                tcomm = ts["Name"].strip()
                # Keyed by thread id: a thread that exits during the window
                # must not turn into a negative count.
                s["per_thread"][(int(t), f"{comm}/{tcomm}")] += int(ts["voluntary_ctxt_switches"]) + int(ts["nonvoluntary_ctxt_switches"])
        except (OSError, KeyError):
            pass
    return s


def measure(name, pids_fn, seconds):
    a, t0 = sample(pids_fn()), time.monotonic()
    time.sleep(seconds)
    b, dt = sample(pids_fn()), time.monotonic() - t0
    # Threads alive at both ends; threads that started during the window
    # count from zero. (A thread that exited is not counted at all.)
    per_tid = {k: (b["per_thread"][k] - a["per_thread"].get(k, 0)) / dt for k in b["per_thread"]}
    wake = collections.Counter()
    for (_, thread), v in per_tid.items():
        wake[thread] += v
    r = {
        "mode": name,
        "seconds": round(dt),
        "cpu_percent_of_one_core": round((b["ticks"] - a["ticks"]) / TICKS / dt * 100, 3),
        "wakeups_per_s": round(sum(wake.values()), 2),
        "rss_mib": round(b["rss_kib"] / 1024, 1),
        "threads": b["threads"],
        "processes": len(pids_fn()),
        "read_syscalls_per_s": round((b["syscr"] - a["syscr"]) / dt, 1),
        "written_kib": round((b["wchar"] - a["wchar"]) / 1024, 1),
        "top_threads": sorted(((k, round(v, 1)) for k, v in wake.items() if v > 0.5), key=lambda kv: -kv[1])[:8],
    }
    print(json.dumps(r), flush=True)
    return r


results = []
cli("start")
pid = json.loads(cli("status", "--json").stdout)["daemon"]["pid"]
d = lambda: [pid]
cli("monitor", "stop")
time.sleep(3)
results.append(measure("daemon, monitoring off", d, secs))
for profile in ("eco", "balanced", "performance"):
    cli("monitor", "profile", profile, "--power-aware", "off")
    cli("monitor", "start")
    time.sleep(8)
    results.append(measure(f"daemon, {profile}", d, secs))

cli("monitor", "profile", "balanced", "--power-aware", "off")
time.sleep(5)
app = subprocess.Popen([f"{B}/sentinel-desktop"], env=GENV, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
ui = UI(pid=app.pid)
ui.doc()
time.sleep(15)
w = lambda: tree(app.pid)
r = measure("window on Overview, monitoring on (balanced)", w, secs)
r["daemon_during"] = measure("  daemon during the same window", d, 1)
results.append(r)
ui.press("button", "Telemetry")
time.sleep(5)
results.append(measure("window on Telemetry/CPU (charts), monitoring on", w, secs))
ui.press("button", "Overview")
cli("monitor", "stop")
time.sleep(8)
results.append(measure("window on Overview, monitoring off", w, secs))
cli("monitor", "start")
time.sleep(5)
ui.press("button", "Minimise")
time.sleep(8)
results.append(measure("window minimised, monitoring on", w, secs))
app.terminate()
app.wait(timeout=10)
cli("stop")
shutil.rmtree(RUN, ignore_errors=True)
json.dump(results, open(f"{work}/bench4.json", "w"), indent=1)
print("DONE")
