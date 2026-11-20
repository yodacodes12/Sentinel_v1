#!/usr/bin/env python3
"""Phase 4 end-to-end scenarios on the real sentinel-desktop window.

The release daemon reads the real /proc and /sys in a throwaway home; the
window is the real Tauri app, driven through AT-SPI (clicks only). Signals
are sent only to this run's own daemon. Results go to results.json.

Usage: window_scenarios.py <repo> <workdir>
"""
import json, os, shutil, signal, socket, subprocess, sys, tempfile, time, traceback

from ui import UI

repo, work = sys.argv[1:3]
B = f"{repo}/target/release"
HOME = f"{work}/home"
shutil.rmtree(HOME, ignore_errors=True)
os.makedirs(f"{HOME}/.config/sentinel")
RUN = tempfile.mkdtemp(prefix="snl-e2e.", dir="/tmp")
os.chmod(RUN, 0o700)
SOCK = f"{RUN}/sentinel/daemon.sock"
# The daemon never sees the session bus here: no notification can pop up.
DENV = {"HOME": HOME, "XDG_RUNTIME_DIR": RUN, "PATH": os.environ["PATH"], "SENTINEL_DAEMON_BIN": f"{B}/sentineld"}
# The window needs the display and the accessibility bus (on the session bus).
GENV = dict(DENV)
for k in ("DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP", "XDG_DATA_DIRS", "LANG"):
    if k in os.environ:
        GENV[k] = os.environ[k]
GENV["WAYLAND_DISPLAY"] = os.path.join("/run/user/%d" % os.getuid(), os.environ.get("WAYLAND_DISPLAY", "wayland-0"))
# A daemon the window starts inherits the session bus, so notifications are
# off in the configuration for this run (the notification test is separate).
open(f"{HOME}/.config/sentinel/config.toml", "w").write("version = 2\nnotifications_enabled = false\n")

results = []
app = None


def cli(*args, check=True):
    r = subprocess.run([f"{B}/sentinel", *args], env=DENV, capture_output=True, text=True, timeout=30)
    if check and r.returncode != 0:
        raise RuntimeError(f"sentinel {' '.join(args)} -> {r.returncode}: {r.stderr}{r.stdout}")
    return r


def status():
    return json.loads(cli("status", "--json", check=False).stdout)


def mon():
    return json.loads(cli("monitor", "status", "--json", check=False).stdout or "{}")


def daemon_pid():
    d = status().get("daemon")
    return d and d["pid"]


def threads(pid):
    for l in open(f"/proc/{pid}/status"):
        if l.startswith("Threads:"):
            return int(l.split()[1])


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


def ask(request):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(5)
    s.connect(SOCK)
    s.sendall(json.dumps({"version": 1, "id": 1, "request": request}).encode() + b"\n")
    buf = b""
    while not buf.endswith(b"\n"):
        c = s.recv(65536)
        if not c:
            break
        buf += c
    s.close()
    return json.loads(buf)


def launch():
    global app
    p = subprocess.Popen([f"{B}/sentinel-desktop"], env=GENV, stdout=subprocess.DEVNULL, stderr=open(f"{work}/desktop.log", "a"))
    app = p
    ui = UI(pid=p.pid, timeout=40)
    ui.wait_text("SYSTEM OVERVIEW", 20)
    return ui


def header(ui):
    """The header's accessible text, from the first landmark."""
    return ui.text().split("Sections")[0]


def scenario(name, fn):
    t0 = time.time()
    try:
        detail = fn() or ""
        results.append({"scenario": name, "result": "pass", "seconds": round(time.time() - t0, 1), "detail": detail})
        print(f"PASS {name}: {detail}", flush=True)
    except Exception as e:
        results.append({"scenario": name, "result": "fail", "seconds": round(time.time() - t0, 1), "detail": f"{type(e).__name__}: {e}", "trace": traceback.format_exc()[-1500:]})
        print(f"FAIL {name}: {e}", flush=True)


PAGES = [
    ("Overview", "SYSTEM OVERVIEW"),
    ("Hardware", "HARDWARE EXPLORER"),
    ("Motherboard", "MOTHERBOARD"),
    ("Telemetry", "CORE TOPOLOGY · LIVE UTILISATION"),
    ("Schematic Library", "MOTHERBOARD DOCUMENTATION"),
    ("Diagnostics DEV", "DIAGNOSTICS"),
    ("Installed Utilities", "INSTALLED UTILITIES"),
    ("Services and Startup", "SERVICES"),
    ("Alerts and Events", "ACTIVE ALERTS"),
    ("Settings", "ALERT THRESHOLDS"),
]

# ------------------------------------------------------------------ run
cli("start")
cli("scan", check=False)  # hardware + utilities, read-only
cli("monitor", "start")
time.sleep(3)
ui = None


def s_startup():
    global ui
    t0 = time.time()
    ui = launch()
    ui.wait_text("Running", 15)
    ui.wait_text("Live · updated", 15)
    h = header(ui).lower()
    assert "active" in h and "on" in h.split("sentinel service")[1], h[:300]
    assert ui.find_all(role="toggle button", name="Turn monitoring off"), "monitoring toggle should show on"
    return f"window ready with live data {time.time() - t0:.1f} s after launch; daemon pid {daemon_pid()}"


def s_navigation():
    times = []
    for rnd in range(3):
        for label, heading in PAGES:
            t0 = time.time()
            ui.press("button", label)
            ui.wait_text(heading, 5)
            times.append(time.time() - t0)
            t = ui.text()
            for bad in ("Unexpected error", "undefined", "NaN", "[object Object]"):
                assert bad not in t, f"{label}: page shows {bad!r}"
    return f"{len(times)} navigations, slowest {max(times):.2f} s, mean {sum(times) / len(times):.2f} s"


def s_telemetry_tabs():
    ui.press("button", "Telemetry")
    ui.wait_text("CORE TOPOLOGY", 5)
    seen = []
    for tab, needle in [("CPU", "UTILISATION HISTORY"), ("Memory", "MEMORY HISTORY"), ("Processes", "PID"), ("Battery", "BAT1"),
                        ("Thermal", "TEMPERATURE"), ("GPU", "GPU"), ("Storage", "THROUGHPUT HISTORY"), ("Network", "THROUGHPUT HISTORY")]:
        ui.press("page tab", tab)
        ui.wait_text(needle, 8)
        seen.append(tab)
    ui.press("page tab", "CPU")
    for w in ("15 min", "1 h", "24 h", "5 min"):
        ui.press("radio button", w)
        time.sleep(0.7)
    t = ui.text()
    assert "No data" not in t.split("UTILISATION HISTORY")[1][:400], "5-minute CPU chart has no data"
    return "tabs: " + ", ".join(seen) + "; chart windows switch"


def s_monitoring_toggle():
    ui.press("button", "Overview")
    ui.wait_text("SYSTEM OVERVIEW", 5)
    ui.press("toggle button", "Turn monitoring off")
    ui.find("toggle button", "Turn monitoring on", timeout=10)
    assert mon()["phase"] == "off"
    ui.wait_text("Monitoring off · no live data", 10)
    ui.press("toggle button", "Turn monitoring on")
    ui.find("toggle button", "Turn monitoring off", timeout=10)
    ui.wait_text("Live · updated", 15)
    assert mon()["phase"] == "active"
    return "off and on from the header; daemon agrees each time"


def s_profile():
    ui.press("button", "Overview")
    ui.wait_text("SYSTEM OVERVIEW", 5)
    out = []
    for label, value in (("Eco", "eco"), ("Performance", "performance"), ("Balanced", "balanced")):
        ui.press("radio button", label)
        end = time.time() + 10
        while time.time() < end and mon().get("sampling", {}).get("selected") != value:
            time.sleep(0.3)
        m = mon()["sampling"]
        assert m["selected"] == value, m
        conf = open(f"{HOME}/.config/sentinel/config.toml").read()
        assert f'power_mode = "{value}"' in conf, conf
        cpu = dict(map(tuple, m["intervals_ms"]))["cpu"] if m["effective"] == value else None
        out.append(f"{label}: cpu every {cpu} ms" if cpu else f"{label}: effective {m['effective']}")
    pid = daemon_pid()
    assert threads(pid) == 2, f"daemon has {threads(pid)} threads after profile changes"
    return "; ".join(out) + "; one monitor thread"


def s_settings():
    ui.press("button", "Settings")
    ui.wait_text("ALERT THRESHOLDS", 5)
    conf = lambda: open(f"{HOME}/.config/sentinel/config.toml").read()
    ui.press("toggle button", "Desktop notifications")
    end = time.time() + 10
    while "notifications_enabled = true" not in conf() and time.time() < end:
        time.sleep(0.3)
    assert "notifications_enabled = true" in conf(), conf()
    assert mon()["notifications"]["enabled"] is True
    ui.press("toggle button", "Desktop notifications")
    end = time.time() + 10
    while "notifications_enabled = false" not in conf() and time.time() < end:
        time.sleep(0.3)
    assert mon()["notifications"]["enabled"] is False
    ui.press("radio button", "Black")
    end = time.time() + 10
    while 'theme = "black"' not in conf() and time.time() < end:
        time.sleep(0.3)
    assert 'theme = "black"' in conf()
    ui.press("radio button", "Dark")
    time.sleep(1.5)
    assert 'theme = "black"' not in conf()
    return "notifications on/off and theme change written to config.toml and applied by the daemon"


def s_daemon_stop():
    ui.press("button", "Telemetry")
    t0 = time.time()
    cli("stop")
    ui.find("toggle button", "Start the Sentinel service", timeout=10)
    ui.wait_text("The Sentinel service is not running", 10)
    took = time.time() - t0
    t = ui.text().lower()
    h = header(ui).lower()
    assert "inactive" in h and "off" in h.split("sentinel service")[1], h[:300]
    assert "live · last update" not in t
    ui.press("button", "Overview")
    ui.wait_text("Stopped", 5)
    ui.wait_text("Monitoring off · no live data", 5)
    return f"header OFF / monitoring Inactive and no live data {took:.1f} s after the daemon stopped"


def s_daemon_restart():
    t0 = time.time()
    cli("start")
    ui.find("toggle button", "Stop the Sentinel service", timeout=10)
    ui.wait_text("Live · updated", 15)
    took = time.time() - t0
    t = ui.text().lower()
    assert "timed out" not in t and "not running" not in t, t[:400]
    return f"reconnected and live again {took:.1f} s after restart; monitoring resumed (preference kept)"


def s_frozen():
    pid = daemon_pid()
    ui.press("button", "Overview")
    ui.wait_text("Live · updated", 10)
    base_threads = threads(app.pid)
    os.kill(pid, signal.SIGSTOP)
    try:
        t0 = time.time()
        ui.wait_text("NO REPLY", 20)
        took = time.time() - t0
        ui.wait_text("Sentinel is not answering", 10)
        h = header(ui).lower()
        assert "unknown" in h, h[:300]
        # The window is not blocked: navigation still responds.
        n0 = time.time()
        ui.press("button", "Hardware")
        ui.wait_text("HARDWARE EXPLORER", 5)
        ui.press("button", "Overview")
        ui.wait_text("SYSTEM OVERVIEW", 5)
        nav = time.time() - n0
        time.sleep(12)  # several poll periods while frozen
        frozen_threads = threads(app.pid)
        from gi.repository import Atspi
        toggle = ui.find("toggle button", contains="Sentinel service")
        assert not toggle.get_state_set().contains(Atspi.StateType.ENABLED), "service toggle must be disabled while unreachable"
    finally:
        os.kill(pid, signal.SIGCONT)
    t0 = time.time()
    ui.find("toggle button", "Stop the Sentinel service", timeout=20)
    ui.wait_text("Live · updated", 20)
    rec = time.time() - t0
    time.sleep(3)
    t = ui.text()
    assert "timed out" not in t.lower(), "a stale timeout banner remained after recovery"
    assert daemon_pid() == pid
    return (f"'NO REPLY' after {took:.1f} s; navigation while frozen {nav:.1f} s; desktop threads {base_threads} -> {frozen_threads} "
            f"while frozen; live again {rec:.1f} s after resume; no leftover error banner")


def s_crash_and_start_from_ui():
    pid = daemon_pid()
    os.kill(pid, signal.SIGKILL)
    ui.find("toggle button", "Start the Sentinel service", timeout=10)
    ui.wait_text("Stopped", 5)
    t0 = time.time()
    ui.press("toggle button", "Start the Sentinel service")
    ui.find("toggle button", "Stop the Sentinel service", timeout=15)
    ui.wait_text("did not shut down cleanly", 10)
    ui.wait_text("Live · updated", 20)
    newpid = daemon_pid()
    assert newpid and newpid != pid
    assert threads(newpid) == 2, threads(newpid)
    return f"after SIGKILL the window said 'Stopped'; started from the header in {time.time() - t0:.1f} s; recovery notice shown; one monitor thread"


def s_alert_flow():
    # A safe, controlled alert: free-space threshold above the real free
    # space. Nothing on the disk changes.
    r = ask({"type": "UpdateConfiguration", "patch": {"alerts": {"storage_free_low_percent": 99.0, "storage_free_critical_percent": 1.0}}})
    assert r["outcome"]["status"] == "ok", r
    ui.press("button", "Alerts and Events")
    ui.wait_text("Low disk space on /", 20)
    ack = ui.find_all(role="button", name="Acknowledge")
    assert ack, "no acknowledge button"
    n_active = len(ack)
    ui.click(ack[0])
    end = time.time() + 10
    while time.time() < end and len(ui.find_all(role="button", name="Acknowledge")) >= n_active:
        time.sleep(0.3)
    acked = [a for a in ask({"type": "GetActiveAlerts"})["outcome"]["response"]["alerts"] if a["acknowledged"]]
    assert acked, "the daemon has no acknowledged alert"
    # Back to defaults: the condition clears at once (no clear time).
    r = ask({"type": "UpdateConfiguration", "patch": {"alerts": {}}})
    assert r["outcome"]["status"] == "ok", r
    ui.wait_text("No active alerts", 20)
    ui.wait_text("Low disk space on /", 10)  # now under history
    hist = ask({"type": "GetAlertHistory", "limit": 50})["outcome"]["response"]["alerts"]
    assert any(a["rule"] == "storage_low" and a["status"] == "resolved" for a in hist)
    return f"{n_active} storage alert(s) raised from a raised threshold, acknowledged in the window, resolved, listed in history"


def s_close_and_relaunch():
    global ui
    pid = daemon_pid()
    # The daemon may be the window's child (it was started from the header);
    # it is meant to outlive the window, so it is not counted.
    procs = [p for p in tree(app.pid) if p != pid]
    ui.press("button", "Close window (Sentinel keeps running if it is on)")
    app.wait(timeout=10)
    end = time.time() + 5
    while time.time() < end and any(os.path.exists(f"/proc/{p}") for p in procs):
        time.sleep(0.2)
    left = [(p, open(f"/proc/{p}/comm").read().strip()) for p in procs if os.path.exists(f"/proc/{p}")]
    assert not left, f"processes left behind: {left}"
    assert daemon_pid() == pid and mon()["phase"] == "active"
    assert threads(pid) == 2
    ui = launch()
    ui.wait_text("Live · updated", 15)
    assert threads(pid) == 2, "a relaunch must not add collectors"
    return f"window and its {len(procs) - 1} WebKit helper processes exited; daemon kept monitoring; relaunch reconnected, still 2 daemon threads"


for name, fn in [
    ("startup with the daemon already running", s_startup),
    ("navigation, 3 rounds over every page", s_navigation),
    ("telemetry tabs and chart windows", s_telemetry_tabs),
    ("monitoring on/off from the header", s_monitoring_toggle),
    ("sampling profile from the Overview", s_profile),
    ("settings persist", s_settings),
    ("daemon stopped while the window is open", s_daemon_stop),
    ("daemon restarted while the window is open", s_daemon_restart),
    ("daemon frozen (slow IPC), then resumed", s_frozen),
    ("daemon crash, restart from the window", s_crash_and_start_from_ui),
    ("alert raised, acknowledged, resolved", s_alert_flow),
    ("close the window, relaunch", s_close_and_relaunch),
]:
    scenario(name, fn)

try:
    app.terminate()
    app.wait(timeout=10)
except Exception:
    pass
cli("stop", check=False)
shutil.rmtree(RUN, ignore_errors=True)
json.dump(results, open(f"{work}/results.json", "w"), indent=2)
print("SUMMARY", sum(r["result"] == "pass" for r in results), "/", len(results), flush=True)
