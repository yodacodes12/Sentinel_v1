#!/usr/bin/env python3
"""Phase 4, workstream D: Sentinel's readings against the kernel's own numbers.

Rates: the mean of Sentinel's 5-minute live series against the kernel
counter difference over the same window (independent arithmetic, same
definition). Levels: Sentinel's latest snapshot against a direct read taken
right after it. Usage: accuracy.py <repo> <workdir> <seconds>
"""
import glob, json, os, shutil, socket, subprocess, sys, tempfile, time

repo, work, window = sys.argv[1], sys.argv[2], float(sys.argv[3])
B = f"{repo}/target/release"
HOME = f"{work}/home"
shutil.rmtree(HOME, ignore_errors=True)
os.makedirs(f"{HOME}/.config/sentinel")
open(f"{HOME}/.config/sentinel/config.toml", "w").write("version = 2\nnotifications_enabled = false\n")
RUN = tempfile.mkdtemp(prefix="snl-a.", dir="/tmp")
os.chmod(RUN, 0o700)
SOCK = f"{RUN}/sentinel/daemon.sock"
ENV = {"HOME": HOME, "XDG_RUNTIME_DIR": RUN, "PATH": os.environ["PATH"], "SENTINEL_DAEMON_BIN": f"{B}/sentineld"}


def cli(*a):
    return subprocess.run([f"{B}/sentinel", *a], env=ENV, capture_output=True, text=True, timeout=30)


def ask(request):
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(10)
    s.connect(SOCK)
    s.sendall(json.dumps({"version": 1, "id": 1, "request": request}).encode() + b"\n")
    buf = b""
    while not buf.endswith(b"\n"):
        c = s.recv(1 << 20)
        if not c:
            break
        buf += c
    s.close()
    out = json.loads(buf)["outcome"]
    assert out["status"] == "ok", out
    return out["response"]


def stat():
    cpus = {}
    for line in open("/proc/stat"):
        if line.startswith("cpu"):
            f = line.split()
            v = list(map(int, f[1:9]))
            cpus[f[0]] = (sum(v), v[3] + v[4])
    return cpus


def netdev():
    out = {}
    for line in open("/proc/net/dev").readlines()[2:]:
        name, rest = line.split(":", 1)
        f = rest.split()
        out[name.strip()] = (int(f[0]), int(f[8]))
    return out


def diskstats():
    out = {}
    for line in open("/proc/diskstats"):
        f = line.split()
        out[f[2]] = (int(f[5]) * 512, int(f[9]) * 512)
    return out


def read(p, scale=1.0):
    try:
        return int(open(p).read().strip()) * scale
    except (OSError, ValueError):
        return None


rows = []


def compare(metric, sentinel, reference, tolerance, unit, how):
    ok = sentinel is not None and reference is not None and abs(sentinel - reference) <= tolerance
    rows.append({"metric": metric, "sentinel": None if sentinel is None else round(sentinel, 3),
                 "reference": None if reference is None else round(reference, 3), "unit": unit,
                 "tolerance": tolerance, "ok": ok, "reference_source": how})
    print(f"{'OK ' if ok else 'BAD'} {metric}: sentinel {sentinel} vs {reference} {unit} (±{tolerance}) [{how}]", flush=True)


try:
    cli("start")
    cli("monitor", "profile", "performance", "--power-aware", "off")
    cli("monitor", "start")
    time.sleep(3)
    t_a, s_a, n_a, d_a = time.time() * 1000, stat(), netdev(), diskstats()
    time.sleep(window)
    t_b, s_b, n_b, d_b = time.time() * 1000, stat(), netdev(), diskstats()

    def series_mean(keys):
        got = ask({"type": "GetTelemetryHistory", "query": {"series": keys, "window": "minutes5"}})["series"]
        out = {}
        for s in got:
            # A point at t is the rate over the interval that ended at t.
            pts = [p["v"] for p in s["points"] if p["v"] is not None and t_a + 1000 <= p["t"] <= t_b]
            out[s["key"]] = (sum(pts) / len(pts) if pts else None, s["unit"], len(pts))
        return out

    ncpu = len([k for k in s_a if k != "cpu"])
    keys = ["cpu.total"] + [f"cpu.core.{i}" for i in range(ncpu)]
    m = series_mean(keys)
    ref = lambda name: 100.0 * (1 - (s_b[name][1] - s_a[name][1]) / (s_b[name][0] - s_a[name][0]))
    compare("CPU total", m["cpu.total"][0], ref("cpu"), 1.5, "%", f"/proc/stat delta over {window:.0f} s; {m['cpu.total'][2]} samples")
    worst = max(abs((m[f"cpu.core.{i}"][0] or 0) - ref(f"cpu{i}")) for i in range(ncpu))
    compare(f"CPU per core, worst of {ncpu}", worst, 0.0, 3.0, "% difference", "/proc/stat per-cpu deltas")
    core_mean = sum(m[f"cpu.core.{i}"][0] for i in range(ncpu)) / ncpu
    compare("CPU total vs mean of cores", m["cpu.total"][0], core_mean, 1.0, "%", "Sentinel's own per-core series")

    ifaces = [i for i in n_a if i != "lo" and n_b[i][0] - n_a[i][0] > 0]
    nm = series_mean([f"net.{i}.{d}" for i in ifaces for d in ("rx", "tx")])
    for i in ifaces:
        for j, d in enumerate(("rx", "tx")):
            delta = (n_b[i][j] - n_a[i][j]) / ((t_b - t_a) / 1000)
            got = nm.get(f"net.{i}.{d}", (None, "", 0))
            compare(f"Network {i} {d}", got[0], delta, max(0.05 * delta, 512), "B/s", "/proc/net/dev counter delta")

    disks = [d for d in d_a if d in d_b and (d_b[d][0] + d_b[d][1]) > (d_a[d][0] + d_a[d][1]) and not d[-1].isdigit() or d.startswith("nvme") and d.endswith("n1")]
    disks = sorted(set(d for d in disks if os.path.exists(f"/sys/block/{d}")))
    dm = series_mean([f"disk.{d}.{x}" for d in disks for x in ("read", "write")])
    for d in disks:
        for j, x in enumerate(("read", "write")):
            delta = (d_b[d][j] - d_a[d][j]) / ((t_b - t_a) / 1000)
            got = dm.get(f"disk.{d}.{x}", (None, "", 0))
            compare(f"Disk {d} {x}", got[0], delta, max(0.05 * delta, 4096), "B/s", "/proc/diskstats sector delta")

    # ---- levels: snapshot, then a direct read
    snap = ask({"type": "GetLatestMetrics"})["snapshot"]
    mi = {l.split(":")[0]: int(l.split()[1]) * 1024 for l in open("/proc/meminfo")}
    mem = snap["memory"]
    compare("Memory total", mem["total_bytes"]["value"], mi["MemTotal"], 0, "B", "/proc/meminfo MemTotal")
    compare("Memory available", mem["available_bytes"]["value"] / 2**20, mi["MemAvailable"] / 2**20, 64, "MiB", "/proc/meminfo MemAvailable (moves)")
    compare("Memory used %", mem["used_percent"]["value"], 100 * (1 - mi["MemAvailable"] / mi["MemTotal"]), 1.5, "%", "MemTotal - MemAvailable")
    compare("Swap used", mem["swap_used_bytes"]["value"] / 2**20, (mi["SwapTotal"] - mi["SwapFree"]) / 2**20, 16, "MiB", "SwapTotal - SwapFree")

    for b in snap["battery"]["batteries"]:
        d = f"/sys/class/power_supply/{b['name']}"
        compare(f"Battery {b['name']} percent", b["percent"]["value"], read(f"{d}/capacity"), 1, "%", f"{d}/capacity")
        compare(f"Battery {b['name']} voltage", b["voltage_v"]["value"], read(f"{d}/voltage_now", 1e-6), 0.05, "V", "voltage_now")
        compare(f"Battery {b['name']} charge now", b["charge_now_ah"]["value"], read(f"{d}/charge_now", 1e-6), 0.02, "Ah", "charge_now")
        state = open(f"{d}/status").read().strip().lower().replace(" ", "_")
        rows.append({"metric": f"Battery {b['name']} state", "sentinel": b["state"]["value"], "reference": state, "ok": b["state"]["value"] == state, "reference_source": "status"})
        print(f"{'OK ' if b['state']['value'] == state else 'BAD'} battery state {b['state']['value']} vs {state}")
        rt = b["runtime"]
        rows.append({"metric": f"Battery {b['name']} runtime", "sentinel": rt, "reference": "state " + state, "ok": (state == "discharging") == (rt["state"] != "not_discharging"), "reference_source": "consistency with status"})

    for t in snap["thermal"]["temperatures"]:
        src = t.get("source", "")
        if src.endswith("_input") and os.path.exists(src):
            compare(f"Temperature {t['chip']} {t.get('label') or t['id']}", t["value_c"].get("value"), read(src, 1e-3), 3.0, "°C", src)
        elif "thermal_zone" in src:
            p = src if src.endswith("temp") else os.path.join(src, "temp")
            compare(f"Temperature {t['id']}", t["value_c"].get("value"), read(p, 1e-3), 3.0, "°C", p)

    for g in (snap.get("gpu") or {}).get("gpus", []):
        dev = glob.glob(f"/sys/bus/pci/devices/{g['pci_address']}")
        if dev:
            vram = read(f"{dev[0]}/mem_info_vram_used")
            if vram is not None and g.get("vram_used_bytes", {}).get("value") is not None:
                compare("GPU VRAM used", g["vram_used_bytes"]["value"] / 2**20, vram / 2**20, 32, "MiB", "mem_info_vram_used")
            gtt = read(f"{dev[0]}/mem_info_gtt_used")
            if gtt is not None and g.get("gtt_used_bytes", {}).get("value") is not None:
                compare("GPU GTT used", g["gtt_used_bytes"]["value"] / 2**20, gtt / 2**20, 64, "MiB", "mem_info_gtt_used")
            hw = glob.glob(f"{dev[0]}/hwmon/hwmon*/temp1_input")
            if hw and g.get("temperature_c", {}).get("value") is not None:
                compare("GPU temperature", g["temperature_c"]["value"], read(hw[0], 1e-3), 3.0, "°C", hw[0])
            busy = read(f"{dev[0]}/gpu_busy_percent")
            rows.append({"metric": "GPU busy", "sentinel": g.get("busy_percent"), "reference": busy, "ok": g.get("busy_percent", {}).get("state") == "ok", "reference_source": "gpu_busy_percent (instantaneous; value not compared)"})

    procs = ask({"type": "GetProcessMetrics"})["processes"]
    pid = json.loads(cli("status", "--json").stdout)["daemon"]["pid"]
    me = next((p for p in procs["processes"] if p["pid"] == pid), None) if procs else None
    if me:
        vmrss = next(int(l.split()[1]) * 1024 for l in open(f"/proc/{pid}/status") if l.startswith("VmRSS"))
        compare("Process RSS (sentineld itself)", me["rss_bytes"] / 2**20, vmrss / 2**20, 2, "MiB", f"/proc/{pid}/status VmRSS")
        threads = next(int(l.split()[1]) for l in open(f"/proc/{pid}/status") if l.startswith("Threads"))
        compare("Process threads (sentineld)", me["threads"], threads, 0, "", f"/proc/{pid}/status Threads")
finally:
    cli("stop")
    shutil.rmtree(RUN, ignore_errors=True)
    json.dump(rows, open(f"{work}/accuracy.json", "w"), indent=1)
print("SUMMARY", sum(r["ok"] for r in rows), "/", len(rows))
