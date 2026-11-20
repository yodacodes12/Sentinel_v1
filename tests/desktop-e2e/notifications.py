#!/usr/bin/env python3
"""Phase 4, workstream F: real desktop notifications, end to end.

A controlled alert: the low-memory threshold is raised to 99 % of memory
available, so the real reading triggers it. Nothing on the system changes.
The daemon talks to the real session bus; dbus-monitor records every
Notify call; gnome-shell's accessibility tree is searched for the banner.

Usage: notifications.py <repo> <workdir>
"""
import json, os, shutil, socket, subprocess, sys, tempfile, time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

repo, work = sys.argv[1:3]
B = f"{repo}/target/release"
HOME = f"{work}/home"
shutil.rmtree(HOME, ignore_errors=True)
os.makedirs(f"{HOME}/.config/sentinel")
RUN = tempfile.mkdtemp(prefix="snl-n.", dir="/tmp")
os.chmod(RUN, 0o700)
SOCK = f"{RUN}/sentinel/daemon.sock"
ENV = {"HOME": HOME, "XDG_RUNTIME_DIR": RUN, "PATH": os.environ["PATH"], "SENTINEL_DAEMON_BIN": f"{B}/sentineld",
       "DBUS_SESSION_BUS_ADDRESS": os.environ["DBUS_SESSION_BUS_ADDRESS"]}
open(f"{HOME}/.config/sentinel/config.toml", "w").write(
    "version = 2\nnotifications_enabled = true\n\n[alerts]\nsustain_s = 5\n")
log = []


def note(msg):
    line = f"[{time.strftime('%H:%M:%S')}] {msg}"
    print(line, flush=True)
    log.append(line)


def cli(*a):
    return subprocess.run([f"{B}/sentinel", *a], env=ENV, capture_output=True, text=True, timeout=30)


def ask(request):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(5)
    s.connect(SOCK)
    s.sendall(json.dumps({"version": 1, "id": 1, "request": request}).encode() + b"\n")
    buf = b""
    while not buf.endswith(b"\n"):
        c = s.recv(1 << 16)
        if not c:
            break
        buf += c
    s.close()
    out = json.loads(buf)["outcome"]
    assert out["status"] == "ok", out
    return out.get("response")


def thresholds(**kw):
    ask({"type": "UpdateConfiguration", "patch": {"alerts": {"sustain_s": 5, **kw}}})


def report():
    return ask({"type": "GetMonitoringStatus"})


def memory_alerts():
    return [a for a in ask({"type": "GetActiveAlerts"})["alerts"] if a["rule"] == "memory_low"]


def notify_calls():
    text = open(f"{work}/dbus.log").read()
    return text.count("member=Notify") , text


def banner_visible(title, timeout=8):
    end = time.time() + timeout
    while time.time() < end:
        desk = Atspi.get_desktop(0)
        for i in range(desk.get_child_count()):
            a = desk.get_child_at_index(i)
            if a is None or a.get_name() != "gnome-shell":
                continue
            stack = [a]
            while stack:
                n = stack.pop()
                try:
                    if title.lower() in (n.get_name() or "").lower():
                        return n.get_role_name()
                    stack.extend(n.get_child_at_index(j) for j in range(n.get_child_count()))
                except Exception:
                    pass
        time.sleep(0.5)
    return None


def wait(what, seconds, cond):
    end = time.time() + seconds
    while not cond():
        if time.time() > end:
            raise TimeoutError(what)
        time.sleep(0.5)


mon = subprocess.Popen(["dbus-monitor", "--session",
                        "type='method_call',interface='org.freedesktop.Notifications',member='Notify'"],
                       stdout=open(f"{work}/dbus.log", "w"), stderr=subprocess.STDOUT)
time.sleep(1)
result = {}
try:
    cli("start")
    cli("monitor", "profile", "balanced", "--power-aware", "off")
    cli("monitor", "start")
    note("daemon started with the real session bus; notifications on; sustain 5 s")

    # 1-3: detection, persistence, delivery
    t0 = time.time()
    thresholds(memory_available_low_percent=99.0, memory_available_critical_percent=1.0)
    wait("memory_low raised", 30, lambda: memory_alerts())
    a = memory_alerts()[0]
    note(f"alert {a['id']} raised {time.time() - t0:.1f} s after the threshold change: {a['title']} / {a['evidence']}")
    stored = subprocess.run(["sqlite3", f"file:{HOME}/.local/share/sentinel/history.db?mode=ro",
                             f"select id,status from alerts where id={a['id']}"], capture_output=True, text=True).stdout.strip() \
        if shutil.which("sqlite3") else None
    wait("delivery recorded", 10, lambda: report()["notifications"]["delivered"] >= 1)
    n = report()["notifications"]
    calls, text = notify_calls()
    note(f"daemon: delivered={n['delivered']} failed={n['failed']} last_ok={n['last_delivery_ok']}; dbus-monitor saw {calls} Notify call(s)")
    shown = banner_visible("Low available memory")
    note(f"gnome-shell accessibility tree shows the banner: {shown!r}")
    result.update(raised=True, notified=a["notified"], delivered=n["delivered"], notify_calls=calls,
                  banner_in_shell=shown, stored=stored, dbus_excerpt=text[-1200:])

    # 7: a continuing alert does not repeat
    time.sleep(40)
    calls2, _ = notify_calls()
    note(f"40 s later, still active: {len(memory_alerts())} alert(s), Notify calls {calls2}")
    result["calls_after_40s"] = calls2

    # resolution is not notified
    thresholds()
    wait("resolved", 60, lambda: not memory_alerts())
    time.sleep(2)
    calls3, _ = notify_calls()
    note(f"resolved after the threshold was restored; Notify calls {calls3}")
    result["calls_after_resolve"] = calls3

    # back within the cooldown: recorded, not notified
    thresholds(memory_available_low_percent=99.0, memory_available_critical_percent=1.0)
    wait("raised again", 30, lambda: memory_alerts())
    b = memory_alerts()[0]
    time.sleep(2)
    calls4, _ = notify_calls()
    note(f"raised again inside the cooldown: alert {b['id']} notified={b['notified']}; Notify calls {calls4}")
    result.update(repeat_notified=b["notified"], calls_after_repeat=calls4)

    # daemon restart with the condition still present
    cli("stop")
    cli("start")
    wait("raised after restart", 40, lambda: memory_alerts())
    c = memory_alerts()[0]
    time.sleep(2)
    calls5, _ = notify_calls()
    note(f"after a daemon restart: alert {c['id']} notified={c['notified']}; Notify calls {calls5}")
    result.update(restart_notified=c["notified"], calls_after_restart=calls5)
    thresholds()
finally:
    cli("stop")
    mon.terminate()
    shutil.rmtree(RUN, ignore_errors=True)
    json.dump({"result": result, "log": log}, open(f"{work}/notify.json", "w"), indent=2)
print(json.dumps({k: v for k, v in result.items() if k != "dbus_excerpt"}, indent=1))
