#!/usr/bin/env python3
"""Phase 6 Schematic Library on the real sentinel-desktop window.

Usage: docs_scenarios.py <repo> <workdir> [--online]

Part 1 (always, offline): Sentinel in a throwaway home scans this computer,
so the identity is this machine's real DMI identity. A synthetic PDF named
for this board is put in the throwaway library; the window must show it as
an exact match with its evidence. No network request may be made: the
daemon's sockets are checked during the search.

Part 2 (--online): a second throwaway home reads the AMD *fixture* machine
(SENTINEL_DISCOVERY_ROOT), turns online discovery on and searches the real
Internet Archive. Only the fictitious fixture identity ("Example Corp
Example Book 14 AMD") is sent; this computer's identity never leaves it.

AT-SPI clicks only; nothing on the system is changed.
"""

import json, os, re, shutil, subprocess, sys, tempfile, time, traceback

from ui import UI

repo, work = sys.argv[1:3]
online = "--online" in sys.argv
B = f"{repo}/target/release"
results = []


def rd(path):
    with open(path) as f:
        return f.read().strip()


def scenario(name, fn):
    t0 = time.time()
    try:
        detail = fn() or ""
        results.append({"scenario": name, "result": "pass", "seconds": round(time.time() - t0, 1), "detail": detail})
        print(f"PASS {name}: {detail}", flush=True)
    except Exception as e:
        results.append({"scenario": name, "result": "fail", "seconds": round(time.time() - t0, 1), "detail": f"{type(e).__name__}: {e}", "trace": traceback.format_exc()[-1500:]})
        print(f"FAIL {name}: {e}", flush=True)


class Env:
    def __init__(self, name, extra=None):
        self.home = f"{work}/{name}"
        shutil.rmtree(self.home, ignore_errors=True)
        os.makedirs(f"{self.home}/.config/sentinel")
        self.run = tempfile.mkdtemp(prefix="snl-p6.", dir="/tmp")
        os.chmod(self.run, 0o700)
        self.env = {"HOME": self.home, "XDG_RUNTIME_DIR": self.run, "PATH": os.environ["PATH"], "SENTINEL_DAEMON_BIN": f"{B}/sentineld", **(extra or {})}
        self.genv = dict(self.env)
        for k in ("DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP", "XDG_DATA_DIRS", "LANG"):
            if k in os.environ:
                self.genv[k] = os.environ[k]
        self.genv["WAYLAND_DISPLAY"] = os.path.join("/run/user/%d" % os.getuid(), os.environ.get("WAYLAND_DISPLAY", "wayland-0"))
        self.app = None

    def config(self, text):
        open(f"{self.home}/.config/sentinel/config.toml", "w").write(text)

    def cli(self, *args):
        return subprocess.run([f"{B}/sentinel", *args], env=self.env, capture_output=True, text=True, timeout=120)

    def daemon_pid(self):
        return int(json.loads(self.cli("status", "--json").stdout)["daemon"]["pid"])

    def launch(self):
        self.app = subprocess.Popen([f"{B}/sentinel-desktop"], env=self.genv, stdout=subprocess.DEVNULL, stderr=open(f"{work}/desktop.log", "a"))
        ui = UI(pid=self.app.pid, timeout=40)
        ui.wait_text("SYSTEM OVERVIEW", 20)
        return ui

    def close(self):
        if self.app:
            self.app.terminate()
            try:
                self.app.wait(timeout=10)
            except Exception:
                self.app.kill()
        self.cli("stop")
        shutil.rmtree(self.run, ignore_errors=True)


def inet_sockets(pid):
    """Sockets of the daemon that are TCP/UDP (not its Unix socket)."""
    inodes = set()
    for fd in os.listdir(f"/proc/{pid}/fd"):
        try:
            t = os.readlink(f"/proc/{pid}/fd/{fd}")
        except OSError:
            continue
        if t.startswith("socket:["):
            inodes.add(t[8:-1])
    net = set()
    for table in ("tcp", "tcp6", "udp", "udp6"):
        try:
            for line in open(f"/proc/net/{table}").read().splitlines()[1:]:
                net.add(line.split()[9])
        except OSError:
            pass
    return inodes & net


def press_contains(ui, text, role="button"):
    for n in ui.find_all(role=role):
        name = n.get_name() or ""
        if text.lower() in name.lower():
            ui.click(n)
            return name
    raise LookupError(f"no {role} containing {text!r}")


# ------------------------------------------------------------- part 1

dmi = {k: rd(f"/sys/class/dmi/id/{k}") for k in ("sys_vendor", "product_name", "board_vendor", "board_name", "board_version")}
local = Env("p6-local")
local.config("version = 2\nnotifications_enabled = false\n")
ui = None


def s_identity():
    global ui
    local.cli("start")
    local.cli("scan", "--hardware")
    lib = f"{local.home}/.local/share/sentinel/documents/library"
    assert os.path.isdir(lib), "library folder not created"
    title = f"{dmi['sys_vendor']} {dmi['product_name']} schematic board {dmi['board_name']} rev {dmi['board_version']}"
    open(f"{lib}/{dmi['board_name']}-{dmi['board_version']}.pdf", "wb").write(b"%PDF-1.4\n<< /Title (" + title.encode() + b") >>\n%%EOF\n")
    open(f"{lib}/unrelated.pdf", "wb").write(b"%PDF-1.4\n<< /Title (Example Vendor Z9999 service manual) >>\n")
    ui = local.launch()
    ui.press("button", "Schematic Library")
    ui.wait_text("MOTHERBOARD DOCUMENTATION", 10)
    t = ui.wait_text("Board identifier", 10)
    t = ui.wait_text(dmi["board_name"], 10)
    for k in ("product_name", "board_name", "board_version"):
        assert dmi[k] in t, f"{k} {dmi[k]!r} not shown"
    assert "Offline: nothing is sent anywhere" in t
    return f"window shows {dmi['sys_vendor']} {dmi['product_name']}, board {dmi['board_name']} rev {dmi['board_version']} from /sys/class/dmi/id"


def s_offline_search():
    pid = local.daemon_pid()
    press_contains(ui, "Search for documentation")
    seen = set()
    end = time.time() + 20
    while time.time() < end:
        seen |= inet_sockets(pid)
        if "Exact match · 1" in ui.text():
            break
        time.sleep(0.2)
    t = ui.text()
    if "exact match · 1" not in t.lower():
        i = t.upper().find("DOCUMENTS")
        raise AssertionError("results not shown: " + t[i:i + 600].replace("\n", " | "))
    assert not seen, f"the daemon opened network sockets while offline: {seen}"
    assert "show 1 document(s) for other hardware" in t.lower() or "unverified" in t.lower()
    return "offline search: exact match from the library, no TCP/UDP socket opened"


def s_detail():
    press_contains(ui, f"{dmi['board_name']}", role="toggle button")
    t = ui.wait_text("What matched", 8)
    for s in ("Contains the board identifier as a whole word.", f"States board revision {dmi['board_version']}", "Your library", "Source (separate from the hardware match)"):
        assert s.lower() in t.lower(), f"detail lacks {s!r}"
    return "detail panel lists the evidence (board ID, revision) and the source separately"


def s_cache_and_clear():
    press_contains(ui, "Search for documentation")
    ui.wait_text("still current", 8)
    press_contains(ui, "Clear results")
    ui.wait_text("cached result(s) forgotten", 8)
    t = ui.wait_text("Not searched yet", 8)
    lib = f"{local.home}/.local/share/sentinel/documents/library"
    assert len(os.listdir(lib)) == 2, "the library must not be touched"
    return "second search reused the cache; clearing forgot results and left the library intact"


def s_explorer():
    press_contains(ui, "Search for documentation")
    try:
        ui.wait_text("Exact match · 1", 15)
    except TimeoutError:
        t = ui.text()
        heads = [l for l in t.splitlines() if l.upper().startswith("DOCUMENTS") or "not searched" in l.lower() or "no documents" in l.lower() or "searching" in l.lower()]
        v = json.loads(local.cli("docs", "--json").stdout)
        raise AssertionError(f"window: {heads[:6]} | daemon: {len(v['candidates'])} candidates, running={v['progress']['running']}")
    ui.press("button", "Hardware")
    ui.wait_text("HARDWARE OVERVIEW", 8)
    for n in ui.find_all(role="button"):
        if re.fullmatch(r"motherboard\s*\d+", (n.get_name() or "").strip().lower()):
            ui.click(n)
            break
    t = ui.wait_text("BOARD DOCUMENTATION", 8)
    assert "1 exact" in t
    press_contains(ui, "Open the Schematic Library")
    ui.wait_text("MOTHERBOARD DOCUMENTATION", 8)
    return "explorer's motherboard category summarises the results and links to the library"


for name, fn in [
    ("real identity from DMI", s_identity),
    ("offline search makes no network connection", s_offline_search),
    ("detail panel evidence", s_detail),
    ("cache reuse and clearing", s_cache_and_clear),
    ("explorer integration", s_explorer),
]:
    scenario(name, fn)
local.close()

# ------------------------------------------------------------- part 2

if online:
    sys.path.insert(0, f"{repo}/tests")
    fx_root = f"{work}/fixture-root"
    # The AMD laptop fixture is built by the Rust test helpers; reuse the one
    # the hardware tests write when SENTINEL_FIXTURE_DUMP is set.
    remote = Env("p6-online", {"SENTINEL_DISCOVERY_ROOT": fx_root})
    remote.config(
        "version = 2\nnotifications_enabled = false\n\n[documents]\nonline_discovery = true\ninternet_archive = true\nmin_request_interval_ms = 1500\nrequest_timeout_s = 20\n"
    )

    def s_online():
        global ui
        assert os.path.isdir(fx_root), f"fixture root missing: {fx_root}"
        remote.cli("start")
        remote.cli("scan", "--hardware")
        ui = remote.launch()
        ui.press("button", "Schematic Library")
        t = ui.wait_text("Example Book 14 AMD", 10)
        assert "Online discovery is on" in t
        t0 = time.time()
        press_contains(ui, "Search for documentation")
        stages = set()
        while time.time() - t0 < 90:
            t = ui.text()
            for s in ("Searching configured providers", "Verifying hardware matches"):
                if s in t:
                    stages.add(s)
            if "Last search:" in t and "Search progress" not in t.upper():
                break
            time.sleep(0.3)
        t = ui.wait_text("Last search:", 30)
        ia = re.search(r"Internet Archive\n.*?\nLast search: ([^\n]+)", t, re.S)
        status = ia.group(1) if ia else "?"
        return f"online search via the real daemon in {time.time() - t0:.1f} s; Internet Archive: {status}; stages seen: {sorted(stages)}"

    scenario("online search with a fictitious identity", s_online)
    remote.close()

# ------------------------------------------------------------- part 3

if "--real-download" in sys.argv:
    # A fixture machine identified as a Lenovo ThinkPad T480 (not this
    # computer). A local catalogue in its library lists Lenovo's official
    # PSREF specification PDF; the configured web catalogue address is a
    # real HTTPS JSON endpoint that is not a catalogue.
    t480 = f"{work}/fixture-t480"
    shutil.rmtree(t480, ignore_errors=True)
    shutil.copytree(f"{work}/fixture-root", t480, symlinks=True)
    for k, v in {"sys_vendor": "LENOVO", "product_name": "20L5CTO1WW", "product_version": "ThinkPad T480", "product_family": "ThinkPad T480", "board_vendor": "LENOVO", "board_name": "20L5CTO1WW", "board_version": "Not Defined"}.items():
        open(f"{t480}/sys/class/dmi/id/{k}", "w").write(v + "\n")
    psref = "https://psref.lenovo.com/syspool/Sys/PDF/ThinkPad/ThinkPad_T480/ThinkPad_T480_Spec.PDF"
    not_a_catalogue = "https://archive.org/advancedsearch.php?q=identifier%3Aiapublicdomain&fl%5B%5D=identifier&rows=1&output=json"
    real = Env("p6-real", {"SENTINEL_DISCOVERY_ROOT": t480})
    real.config(f"version = 2\nnotifications_enabled = false\n\n[documents]\nonline_discovery = true\ninternet_archive = false\ncatalogue_urls = [\"{not_a_catalogue}\"]\n")

    def s_real_download():
        global ui
        real.cli("start")
        real.cli("scan", "--hardware")
        lib = f"{real.home}/.local/share/sentinel/documents/library"
        cat = {"format": "sentinel-document-catalogue", "version": 1, "publisher": "local test list", "documents": [{"title": "ThinkPad T480 PSREF Specifications", "url": psref, "manufacturer": "Lenovo", "models": ["ThinkPad T480"]}]}
        json.dump(cat, open(f"{lib}/lenovo.sentinel-catalogue.json", "w"))
        ui = real.launch()
        ui.press("button", "Schematic Library")
        ui.wait_text("ThinkPad T480", 10)
        press_contains(ui, "Search for documentation")
        t = ui.wait_text("High-confidence match · 1", 60)
        assert "not a document catalogue" in t, "the real web catalogue address should fail as not a catalogue"
        press_contains(ui, "ThinkPad T480 PSREF", role="toggle button")
        ui.wait_text("Manufacturer site", 8)
        t0 = time.time()
        ui.press("button", "Download")
        ui.wait_text("Downloaded copy available", 90)
        took = time.time() - t0
        dl = f"{real.home}/.local/share/sentinel/documents/downloads"
        files = [f for f in os.listdir(dl) if not f.startswith(".")]
        assert len(files) == 1 and re.fullmatch(r"[0-9a-f]{64}\.pdf", files[0]), files
        body = open(f"{dl}/{files[0]}", "rb").read()
        assert body.startswith(b"%PDF-"), "stored file is not a PDF"
        mode = os.stat(f"{dl}/{files[0]}").st_mode & 0o777
        press_contains(ui, "Remove downloaded copy")
        ui.wait_text("downloaded copy was removed", 10)
        assert os.listdir(dl) == [], f"files left after removal: {os.listdir(dl)}"
        return (f"window → daemon → psref.lenovo.com: downloaded {len(body)} bytes in {took:.1f} s as {files[0][:12]}….pdf (mode {oct(mode)}), "
                f"classed high-confidence from the manufacturer's site; the real web catalogue address failed as 'not a document catalogue' "
                f"while other results stayed; removal deleted the copy")

    scenario("real download through the window and daemon", s_real_download)
    real.close()

# ------------------------------------------------------------- part 4

if "--web-catalogue" in sys.argv:
    # A valid catalogue served over real HTTPS (any echo or static host),
    # listing Lenovo's official T480 PDF; fixture identity ThinkPad T480.
    cat_url = sys.argv[sys.argv.index("--web-catalogue") + 1]
    t480 = f"{work}/fixture-t480"
    web = Env("p6-web", {"SENTINEL_DISCOVERY_ROOT": t480})
    web.config(f"version = 2\nnotifications_enabled = false\n\n[documents]\nonline_discovery = true\ninternet_archive = false\ncatalogue_urls = [\"{cat_url}\"]\n")

    def s_web_catalogue():
        global ui
        assert os.path.isdir(t480), "run with --real-download first (it builds the T480 fixture)"
        web.cli("start")
        web.cli("scan", "--hardware")
        lib = f"{web.home}/.local/share/sentinel/documents/library"
        assert os.listdir(lib) == [], "no local catalogue: the entry must come from the web address"
        ui = web.launch()
        ui.press("button", "Schematic Library")
        ui.wait_text("ThinkPad T480", 10)
        press_contains(ui, "Search for documentation")
        t = ui.wait_text("High-confidence match · 1", 60)
        assert "not a document catalogue" not in t and "Provider unavailable" not in t
        out = json.loads(web.cli("docs", "--json").stdout)
        c = out["candidates"][0]
        assert c["source"]["provider_id"] == "catalogue" and c["source"]["trust"] == "official_this_manufacturer", c["source"]
        outcome = [p for p in out["sessions"][0]["providers"] if p["provider_id"] == "catalogue"][0]
        press_contains(ui, "ThinkPad T480 PSREF", role="toggle button")
        ui.press("button", "Download")
        ui.wait_text("Downloaded copy available", 90)
        dl = f"{web.home}/.local/share/sentinel/documents/downloads"
        files = os.listdir(dl)
        assert len(files) == 1 and open(f"{dl}/{files[0]}", "rb").read(5) == b"%PDF-", files
        press_contains(ui, "Remove downloaded copy")
        ui.wait_text("downloaded copy was removed", 10)
        assert os.listdir(dl) == []
        return f"catalogue fetched from a real HTTPS address by the daemon ({outcome['state']}, {outcome['results']} entry, {outcome['requests']} requests); official entry high-confidence, downloaded from psref.lenovo.com in the window, then removed"

    scenario("valid web catalogue end to end", s_web_catalogue)
    web.close()

if ui is not None and ui.stale_cache_hits:
    print("NOTE text seen only after clearing the AT-SPI client cache:", ui.stale_cache_hits, flush=True)
json.dump(results, open(f"{work}/docs_results.json", "w"), indent=2)
print("SUMMARY", sum(r["result"] == "pass" for r in results), "/", len(results), flush=True)
