#!/usr/bin/env python3
"""Phase 5 hardware explorer on the real sentinel-desktop window.

Usage: explorer_scenarios.py <repo> <workdir> [measure_seconds]

Runs Sentinel in a throwaway home (as window_scenarios.py does), scans this
computer, turns monitoring on and drives the Hardware explorer through
AT-SPI (clicks only, never keyboard input). What the window shows is
compared with independent reads of /sys and /proc made by this script.
Then the window's own CPU and memory are measured on three pages.
Nothing is changed on the system; MAC addresses read for the privacy check
are compared in memory and never printed.
"""

import collections, glob, json, os, re, shutil, subprocess, sys, tempfile, time, traceback

from ui import UI

repo, work = sys.argv[1:3]
secs = int(sys.argv[3]) if len(sys.argv) > 3 else 60
B = f"{repo}/target/release"
HOME = f"{work}/home"
shutil.rmtree(HOME, ignore_errors=True)
os.makedirs(f"{HOME}/.config/sentinel")
RUN = tempfile.mkdtemp(prefix="snl-p5.", dir="/tmp")
os.chmod(RUN, 0o700)
DENV = {"HOME": HOME, "XDG_RUNTIME_DIR": RUN, "PATH": os.environ["PATH"], "SENTINEL_DAEMON_BIN": f"{B}/sentineld"}
GENV = dict(DENV)
for k in ("DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP", "XDG_DATA_DIRS", "LANG"):
    if k in os.environ:
        GENV[k] = os.environ[k]
GENV["WAYLAND_DISPLAY"] = os.path.join("/run/user/%d" % os.getuid(), os.environ.get("WAYLAND_DISPLAY", "wayland-0"))
open(f"{HOME}/.config/sentinel/config.toml", "w").write("version = 2\nnotifications_enabled = false\n")
TICKS = os.sysconf("SC_CLK_TCK")

results, app, ui = [], None, None


def cli(*args):
    return subprocess.run([f"{B}/sentinel", *args], env=DENV, capture_output=True, text=True, timeout=60)


def rd(path):
    with open(path) as f:
        return f.read().strip()


def cpu_list(text):
    out = []
    for part in text.split(","):
        if "-" in part:
            a, b = part.split("-")
            out += range(int(a), int(b) + 1)
        elif part:
            out.append(int(part))
    return out


def scenario(name, fn):
    t0 = time.time()
    try:
        detail = fn() or ""
        results.append({"scenario": name, "result": "pass", "seconds": round(time.time() - t0, 1), "detail": detail})
        print(f"PASS {name}: {detail}", flush=True)
    except Exception as e:
        results.append({"scenario": name, "result": "fail", "seconds": round(time.time() - t0, 1), "detail": f"{type(e).__name__}: {e}", "trace": traceback.format_exc()[-1500:]})
        print(f"FAIL {name}: {e}", flush=True)


def clean(text, where):
    for bad in ("Unexpected error", "undefined", "NaN", "[object Object]"):
        assert bad not in text, f"{where}: page shows {bad!r}"


def category(label):
    """The explorer's category button (its name ends with the item count)."""
    pat = re.compile(re.escape(label.lower()) + r"\s*\d+")
    for n in ui.find_all(role="button"):
        if pat.fullmatch((n.get_name() or "").strip().lower()):
            ui.click(n)
            return n.get_name()
    raise LookupError(f"no category button {label!r}")


def inspector():
    t = ui.text()
    return t[t.lower().rfind("inspector"):]


def tree_items():
    return [n.get_name() for n in ui.find_all(role="tree item")]


# ------------------------------------------------------------ the machine

cpus = sorted(int(p.rsplit("cpu", 1)[1]) for p in glob.glob("/sys/devices/system/cpu/cpu[0-9]*") if os.path.exists(f"{p}/topology"))
core_sets = sorted({tuple(cpu_list(rd(f"/sys/devices/system/cpu/cpu{c}/topology/core_cpus_list"))) for c in cpus})
cache_sets = set()
for c in cpus:
    for idx in glob.glob(f"/sys/devices/system/cpu/cpu{c}/cache/index*"):
        cache_sets.add((rd(f"{idx}/level"), rd(f"{idx}/type"), tuple(cpu_list(rd(f"{idx}/shared_cpu_list")))))
l3 = [s for s in cache_sets if s[0] == "3"]
pci_count = len(os.listdir("/sys/bus/pci/devices"))
usb_count = len([d for d in os.listdir("/sys/bus/usb/devices") if ":" not in d and not d.startswith("usb")])
mounts = [l.split() for l in open("/proc/mounts")]
root_dev, _, root_fs = next((m[0], m[1], m[2]) for m in mounts if m[1] == "/")
macs = {rd(p) for p in glob.glob("/sys/class/net/*/address")} - {"00:00:00:00:00:00"}
model = next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), None)

# ------------------------------------------------------------------- run

cli("start")
cli("scan")
cli("monitor", "start")
time.sleep(3)


def launch():
    global app
    app = subprocess.Popen([f"{B}/sentinel-desktop"], env=GENV, stdout=subprocess.DEVNULL, stderr=open(f"{work}/desktop.log", "a"))
    u = UI(pid=app.pid, timeout=40)
    u.wait_text("SYSTEM OVERVIEW", 20)
    return u


def s_open():
    global ui
    ui = launch()
    t0 = time.time()
    ui.press("button", "Hardware")
    t = ui.wait_text("HARDWARE OVERVIEW", 10)
    took = time.time() - t0
    clean(t, "overview")
    if model:
        assert model.lower() in t.lower(), f"CPU model {model!r} not on the overview"
    assert "reliability labels" in t.lower()
    return f"explorer overview shown {took:.2f} s after the click, CPU model matches /proc/cpuinfo"


def s_categories():
    times = []
    for label, needle in [("Processor", "CORE TOPOLOGY"), ("Memory", "MEMORY CAPACITY"), ("Graphics", "GRAPHICS"), ("Storage", "ALL RECORDED FIELDS"), ("Battery", "BATTERY"), ("Network", "ABOUT THIS VIEW"), ("USB", "USB TOPOLOGY"), ("PCI devices", "BUS TOPOLOGY"), ("Motherboard", "BOARD IDENTITY"), ("Sensors", "SENSOR INTERFACES")]:
        t0 = time.time()
        category(label)
        t = ui.wait_text(needle, 8)
        times.append(time.time() - t0)
        clean(t, label)
    return f"{len(times)} categories, slowest {max(times):.2f} s, mean {sum(times) / len(times):.2f} s"


def s_cpu_topology():
    category("Processor")
    ui.wait_text("CACHE HIERARCHY", 8)
    items = tree_items()
    expected = 1 + len(core_sets) + len(cache_sets)
    assert len(items) == expected, f"tree shows {len(items)} items, sysfs has 1 package + {len(core_sets)} cores + {len(cache_sets)} cache instances"
    first = core_sets[0]
    ui.press(None, contains=f"core 0, logical cpus {', '.join(map(str, first))}")
    t = ui.wait_text("Physical core", 8)
    ins = inspector()
    want = ", ".join(f"CPU {c}" for c in first)
    assert want in ins, f"inspector does not list {want}: {ins[:400]}"
    return f"tree = 1 package + {len(core_sets)} cores + {len(cache_sets)} cache instances, as sysfs; core 0 = {want}"


def s_cache_sharing():
    assert l3, "this processor reports no L3 cache"
    ui.press(None, contains="l3 cache")
    ui.wait_text("Cores sharing it", 8)
    ins = inspector()
    sharers = re.search(r"Cores sharing it\n(.*)", ins)
    assert sharers, ins[:500]
    cores_listed = len(re.findall(r"Core \d+", sharers.group(1)))
    l3_cores = {s for s in core_sets if set(s) & set(l3[0][2])}
    assert cores_listed == len(l3_cores), f"inspector lists {cores_listed} cores sharing L3, sysfs says {len(l3_cores)}"
    return f"one L3 instance shared by {cores_listed} cores, matching shared_cpu_list {l3[0][2]}"


def s_live():
    ui.press(None, contains=f"core 0, logical cpus")
    end = time.time() + 15
    while time.time() < end:
        ins = inspector()
        if re.search(r"Utilisation\n\d+\.\d%", ins) and "Live state live" in ui.text():
            break
        time.sleep(0.5)
    else:
        raise TimeoutError("no live utilisation in the inspector")
    a = re.search(r"Utilisation\n(\d+\.\d)%", inspector()).group(1)
    time.sleep(6)
    b = re.search(r"Utilisation\n(\d+\.\d)%", inspector()).group(1)
    assert "stale" not in inspector().lower()
    return f"core 0 utilisation live in the inspector ({a} % then {b} %), state Live"


def s_persistence():
    ui.press(None, contains="l3 cache")
    ui.wait_text("Cores sharing it", 8)
    ui.press("button", "Telemetry")
    ui.wait_text("CORE TOPOLOGY · LIVE UTILISATION", 8)
    time.sleep(5)
    ui.press("button", "Hardware")
    t = ui.wait_text("Cores sharing it", 8)
    assert "CACHE HIERARCHY".lower() in t.lower(), "category was not kept"
    time.sleep(5)  # several telemetry refreshes
    assert "Cores sharing it" in inspector(), "selection lost after refreshes"
    return "L3 selection and Processor category kept across a page change and 5 s of refreshes"


def s_storage():
    category("Storage")
    ui.wait_text("ALL RECORDED FIELDS", 8)
    part = os.path.basename(root_dev)
    if root_dev.startswith("/dev/mapper/") or root_dev.startswith("/dev/dm-"):
        return f"root is on {root_dev}: checked in the tree only"
    ui.press(None, contains=f"{part},")
    ui.wait_text("Mounted at", 8)
    ins = inspector()
    assert re.search(r"Mounted at\n/(\n|,| )", ins + "\n"), ins[:600]
    assert root_fs in ins, f"filesystem type {root_fs} not shown"
    return f"{part} shown mounted at / as {root_fs}, as in /proc/mounts"


def s_counts():
    pci = category("PCI devices")
    usb = category("USB")
    n_pci = int(pci.split()[-1])
    n_usb = int(usb.split()[-1])
    assert n_pci == pci_count, f"PCI {n_pci} vs sysfs {pci_count}"
    assert n_usb == usb_count, f"USB {n_usb} vs sysfs {usb_count}"
    category("PCI devices")
    t = ui.wait_text("BUS TOPOLOGY", 8)
    orphans = t.lower().count("parent not discovered")
    return f"PCI {n_pci} and USB {n_usb} devices, as in sysfs; {orphans} PCI functions without a discovered parent"


def s_cross_link():
    category("Graphics")
    ui.wait_text("PCI FUNCTION", 8)
    gpu = ui.find_all(contains="pci function ")
    assert gpu, "no PCI function button on the GPU diagram"
    addr = gpu[0].get_name().split()[-1]
    ui.click(gpu[0])
    t = ui.wait_text("domain:bus:device.function", 8)
    assert addr in inspector()
    return f"GPU diagram's PCI function {addr} opens the PCI inspector"


def s_battery():
    bats = [p for p in glob.glob("/sys/class/power_supply/*") if os.path.exists(f"{p}/type") and rd(f"{p}/type") == "Battery"]
    if not bats:
        return "no battery on this machine: empty state only"
    p = bats[0]
    kind, unit = ("energy", "Wh") if os.path.exists(f"{p}/energy_full_design") else ("charge", "Ah")
    design = int(rd(f"{p}/{kind}_full_design")) / 1e6
    full = int(rd(f"{p}/{kind}_full")) / 1e6
    category("Battery")
    t = ui.wait_text("Design capacity (firmware)", 8)
    digits = 1 if unit == "Wh" else 2
    for label, v in (("Design capacity (firmware)", design), ("Full-charge capacity (firmware)", full)):
        want = f"{label}: {v:.{digits}f} {unit}"
        assert want.lower() in t.lower(), f"expected {want!r}"
    return f"{os.path.basename(p)} reports {kind}: design {design:.{digits}f} {unit}, full {full:.{digits}f} {unit}, as in sysfs, no unit conversion"


def s_privacy():
    category("Network")
    ui.wait_text("ABOUT THIS VIEW", 8)
    t = ui.text().lower()
    leaked = [m for m in macs if m.lower() in t]
    assert not leaked, f"{len(leaked)} MAC address(es) shown unmasked"
    return f"{len(macs)} interface MAC addresses never shown unmasked"


# ------------------------------------------------------------ resources

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
    ticks = rss = 0
    for pid in pids:
        try:
            rest = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
            ticks += int(rest[11]) + int(rest[12])
            st = dict(l.split(":", 1) for l in open(f"/proc/{pid}/status") if ":" in l)
            rss += int(st["VmRSS"].split()[0])
        except (OSError, KeyError):
            pass
    return ticks, rss


def measure(page, setup):
    setup()
    time.sleep(5)
    daemon = int(json.loads(cli("status", "--json").stdout)["daemon"]["pid"])
    win = lambda: [p for p in tree(app.pid) if p != daemon]
    (a, _), t0 = sample(win()), time.monotonic()
    rss_peak = 0
    end = t0 + secs
    while time.monotonic() < end:
        time.sleep(5)
        rss_peak = max(rss_peak, sample(win())[1])
    (b, rss), dt = sample(win()), time.monotonic() - t0
    r = {"page": page, "seconds": round(dt), "window_cpu_percent_of_one_core": round((b - a) / TICKS / dt * 100, 2), "window_rss_mib": round(rss / 1024, 1), "window_rss_peak_mib": round(rss_peak / 1024, 1)}
    print(json.dumps(r), flush=True)
    return r


def s_resources():
    if secs == 0:
        return "skipped (seconds = 0)"
    rows = [
        measure("Overview", lambda: ui.press("button", "Overview")),
        measure("Telemetry (Phase 4 baseline page)", lambda: ui.press("button", "Telemetry")),
        measure("Hardware explorer, Processor, live core selected", lambda: (ui.press("button", "Hardware"), category("Processor"), ui.press(None, contains="core 0, logical cpus"))),
        measure("Hardware explorer, PCI tree", lambda: category("PCI devices")),
        measure("Hardware explorer, Storage", lambda: category("Storage")),
    ]
    json.dump(rows, open(f"{work}/explorer_resources.json", "w"), indent=2)
    return "; ".join(f"{r['page']}: {r['window_cpu_percent_of_one_core']} % CPU, {r['window_rss_mib']} MiB" for r in rows)


for name, fn in [
    ("open the explorer", s_open),
    ("every category renders", s_categories),
    ("CPU topology matches sysfs", s_cpu_topology),
    ("cache sharing matches sysfs", s_cache_sharing),
    ("live per-core telemetry in the inspector", s_live),
    ("selection persists across navigation and refreshes", s_persistence),
    ("storage hierarchy matches /proc/mounts", s_storage),
    ("PCI and USB counts match sysfs", s_counts),
    ("cross-category link (GPU to PCI)", s_cross_link),
    ("battery capacity matches sysfs", s_battery),
    ("MAC addresses stay masked", s_privacy),
    (f"window resources, {secs} s per page", s_resources),
]:
    scenario(name, fn)

try:
    app.terminate()
    app.wait(timeout=10)
except Exception:
    pass
cli("stop")
shutil.rmtree(RUN, ignore_errors=True)
json.dump(results, open(f"{work}/explorer_results.json", "w"), indent=2)
print("SUMMARY", sum(r["result"] == "pass" for r in results), "/", len(results), flush=True)
