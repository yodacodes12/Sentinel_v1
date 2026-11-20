// Phase 4: hook behaviour of the real frontend in a real browser engine.
//
// The built frontend (apps/desktop/dist) runs in headless Firefox (WebDriver
// BiDi, no extra driver needed) with a scripted stand-in for the Tauri
// bridge, so a slow daemon and typing can be produced at will. Checks:
//   1. an unsaved threshold edit survives the configuration being re-read
//      every 5 s (it used to be reset), and saving sends exactly that value;
//   2. an emptied field blocks saving instead of becoming 0;
//   3. with a daemon that takes 9 s to answer, background requests never
//      overlap (at most one in flight per command);
//   4. every page renders with no uncaught exception or console error, and
//      an idle window polls once per period, not once per component.
//
// Usage: node tests/desktop-e2e/browser_settings.mjs   (after npm run build)
// Nothing outside a temporary Firefox profile is touched; no window opens.

import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { createServer } from "node:http";
import { homedir, tmpdir } from "node:os";
import { extname, join, normalize } from "node:path";

// Optional argument: another build to test (e.g. an older one).
const dist = process.argv[2] ? new URL(`file://${process.argv[2].replace(/\/?$/, "/")}`).pathname : new URL("../../apps/desktop/dist/", import.meta.url).pathname;
if (!existsSync(join(dist, "index.html"))) throw new Error("build the frontend first: npm run build");

// ------------------------------------------------------------ static server
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".png": "image/png" };
const server = createServer((req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname)).replace(/^\/+/, "");
  const file = join(dist, path || "index.html");
  if (!file.startsWith(dist) || !existsSync(file)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" }).end(readFileSync(file));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const url = `http://127.0.0.1:${server.address().port}/`;

// ---------------------------------------------------------- the fake bridge
// Runs in the page before the app. Shapes copied from a real daemon.
const preload = `() => {
  const thresholds = {"cpu_high_percent":90.0,"cpu_high_duration_s":120,"load_per_cpu_high":2.0,"memory_available_low_percent":10.0,"memory_available_critical_percent":5.0,"swap_activity_pages_per_s":500.0,"battery_low_percent":15.0,"battery_critical_percent":5.0,"battery_health_warning_percent":60.0,"storage_free_low_percent":10.0,"storage_free_critical_percent":5.0,"disk_busy_high_percent":90.0,"sustain_s":60,"cooldown_s":600};
  let config = {"version":2,"monitoring_enabled":false,"power_mode":"balanced","notifications_enabled":true,"theme":"dark","power_aware_sampling":true,"alerts":thresholds,"retention":{"detailed_hours":24,"hourly_days":30,"alert_days":90}};
  const daemon = () => ({"app_version":"0.1.0","protocol_version":1,"pid":4242,"state":"running","started_at":1795070150,"uptime_secs":60,"monitoring":{"state":"stopped","engine_available":true,"note":""},"operating_mode":config.power_mode,"config_path":"/home/u/.config/sentinel/config.toml","storage_available":true,"recovered_from_unclean_shutdown":false});
  const report = () => ({"phase":"off","since":0,"sampling":{"selected":"balanced","effective":"balanced","power_aware":true,"reason":null,"intervals_ms":[["cpu",2000]]},"collectors":[],"last_sample_at":null,"error":null,"enabled_preference":false,"history_available":true,"notifications":{"enabled":true,"last_delivery_ok":null,"last_error":null,"delivered":0,"failed":0}});
  // A fresh object on every read, as JSON from IPC is.
  const view = () => JSON.parse(JSON.stringify({"config":config,"path":"/home/u/.config/sentinel/config.toml","source":"daemon","warning":null}));
  const T = (window.__test = { calls: {}, inflight: {}, max: {}, updates: [], slow: {} });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const answer = (cmd, args) => {
    switch (cmd) {
      case "get_overview": return {"client_version":"0.1.0","running":true,"ipc":"connected","ipc_detail":null,"socket_path":"/run/user/1000/sentinel/daemon.sock","daemon":daemon(),"operating_mode":config.power_mode,"config_path":"/home/u/.config/sentinel/config.toml","log_path":"/home/u/.local/state/sentinel/logs/daemon.log"};
      case "get_configuration": return view();
      case "update_configuration":
        T.updates.push(JSON.parse(JSON.stringify(args.patch)));
        config = { ...config, ...args.patch, alerts: { ...config.alerts, ...(args.patch.alerts || {}) } };
        return view();
      case "get_metrics": return { monitoring: report(), snapshot: null };
      case "get_active_alerts": case "get_alert_history": return [];
      case "get_inventory": return {"inventory":null,"origin":null,"source":"daemon","progress":null,"warning":null};
      case "get_utilities": return {"catalog":null,"origin":null,"source":"daemon","progress":null,"warning":null};
      default: throw { kind: "unknown", message: "not part of this test: " + cmd };
    }
  };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback: () => 0,
    unregisterCallback: () => {},
    invoke: async (cmd, args) => {
      T.calls[cmd] = (T.calls[cmd] || 0) + 1;
      T.inflight[cmd] = (T.inflight[cmd] || 0) + 1;
      T.max[cmd] = Math.max(T.max[cmd] || 0, T.inflight[cmd]);
      try {
        if (T.slow[cmd]) await sleep(T.slow[cmd]);
        return answer(cmd, args);
      } finally {
        T.inflight[cmd] -= 1;
      }
    },
  };
}`;

// ------------------------------------------------------------------ Firefox
// Snap-packaged Firefox can only use profiles under its own directory.
const snapDir = join(homedir(), "snap", "firefox", "common");
const profile = mkdtempSync(join(existsSync(snapDir) ? snapDir : tmpdir(), "sentinel-ff-"));
const ff = spawn("firefox", ["--headless", "--no-remote", "--profile", profile, "--remote-debugging-port", "0", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
const wsUrl = await new Promise((resolve, reject) => {
  let err = "";
  const timer = setTimeout(() => reject(new Error("Firefox did not start BiDi:\n" + err)), 30_000);
  ff.stderr.on("data", (d) => {
    err += d;
    const m = /WebDriver BiDi listening on (ws:\/\/\S+)/.exec(err);
    if (m) {
      clearTimeout(timer);
      resolve(m[1]);
    }
  });
});
const ws = new WebSocket(`${wsUrl}/session`);
await new Promise((r, j) => ((ws.onopen = r), (ws.onerror = j)));
let nextId = 1;
const pending = new Map();
const consoleErrors = [];
ws.onmessage = (e) => {
  const msg = JSON.parse(e.data);
  if (msg.method === "log.entryAdded" && msg.params.level === "error") consoleErrors.push(msg.params.text);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.type === "error" ? reject(new Error(`${msg.error}: ${msg.message}`)) : resolve(msg.result);
  }
};
const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });

const results = [];
let failed = false;
const check = (name, ok, detail) => {
  results.push({ name, ok, detail });
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}: ${detail}`);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

try {
  await send("session.new", { capabilities: {} });
  await send("session.subscribe", { events: ["log.entryAdded"] });
  const { contexts } = await send("browsingContext.getTree", {});
  const context = contexts[0].context;
  await send("script.addPreloadScript", { functionDeclaration: preload });
  await send("browsingContext.navigate", { context, url, wait: "complete" });
  const js = async (expression) => {
    const r = await send("script.evaluate", { expression, target: { context }, awaitPromise: true });
    if (r.type === "exception") throw new Error(r.exceptionDetails.text);
    return r.result.value;
  };
  const helpers = `
    const byText = (sel, text) => [...document.querySelectorAll(sel)].find((e) => e.textContent.trim().toLowerCase() === text.toLowerCase());
    const field = (label) => [...document.querySelectorAll("label")].find((l) => l.textContent.includes(label))?.querySelector("input");
    const type = (input, value) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    };
    const save = () => byText("button", "Save thresholds");`;
  await sleep(2_000);

  // 4. Every page, twice; then an idle minute's worth of polling.
  const pages = ["Overview", "Hardware", "Motherboard", "Telemetry", "Schematic Library DEV", "Diagnostics DEV", "Installed Utilities", "Services and Startup", "Alerts and Events", "Settings"];
  const rendered = [];
  for (let round = 0; round < 2; round++) {
    for (const page of pages) {
      const ok = await js(`(() => { ${helpers} const b = [...document.querySelectorAll('nav[aria-label="Sections"] button')].find((e) => e.textContent.replace(/\\s+/g, " ").trim().toLowerCase().startsWith(${JSON.stringify(page.split(" ")[0].toLowerCase())})); if (!b) return false; b.click(); return true; })()`);
      await sleep(400);
      const main = await js(`(() => document.querySelector("main")?.textContent.length ?? 0)()`);
      rendered.push(`${page}:${ok && main > 0 ? "ok" : "EMPTY"}`);
    }
  }
  await js(`(() => { ${helpers} byText("button", "Overview").click(); return true; })()`);
  await sleep(1_000);
  const c0 = await js(`(() => JSON.stringify(window.__test.calls))()`);
  await sleep(20_000);
  const c1 = await js(`(() => JSON.stringify(window.__test.calls))()`);
  const a = JSON.parse(c0), b = JSON.parse(c1);
  const rate = Object.fromEntries(Object.keys(b).map((k) => [k, (b[k] || 0) - (a[k] || 0)]));
  check("every page renders, twice", rendered.every((r) => r.endsWith(":ok")), rendered.filter((r) => !r.endsWith(":ok")).join(", ") || `${rendered.length} page views`);
  // Monitoring is off here: status every 5 s, so about 4 calls each in 20 s.
  check("an idle window polls once per period", (rate.get_overview ?? 0) <= 5 && (rate.get_metrics ?? 0) <= 5 && (rate.get_configuration ?? 0) <= 5, `calls in 20 s: ${JSON.stringify(rate)}`);
  await js(`(() => { ${helpers} byText("button", "Settings").click(); return true; })()`);
  await sleep(1_000);

  // 1. An edit survives re-reads of the configuration.
  await js(`(() => { ${helpers} type(field("Sustained CPU usage"), "77"); return true; })()`);
  const before = await js(`(() => window.__test.calls.get_configuration)()`);
  await sleep(12_000);
  const after = await js(`(() => window.__test.calls.get_configuration)()`);
  const kept = await js(`(() => { ${helpers} return field("Sustained CPU usage").value + "|" + save().disabled; })()`);
  check("unsaved edit survives refreshes", kept === "77|false", `${after - before} configuration re-reads in 12 s; field shows ${kept.split("|")[0]}, Save enabled: ${kept.split("|")[1] === "false"}`);

  // 2. An emptied field blocks saving.
  const empty = await js(`(() => { ${helpers} type(field("Sustained CPU usage"), ""); return new Promise((r) => setTimeout(() => r(field("Sustained CPU usage").value + "|" + save().disabled + "|" + document.body.textContent.includes("needs a number")), 300)); })()`);
  check("an empty threshold cannot be saved", empty === "|true|true", `value "${empty.split("|")[0]}", Save disabled ${empty.split("|")[1]}, hint shown ${empty.split("|")[2]}`);

  // ... and saving sends what was typed.
  await js(`(() => { ${helpers} type(field("Sustained CPU usage"), "85"); return true; })()`);
  await sleep(300);
  await js(`(() => { ${helpers} save().click(); return true; })()`);
  await sleep(6_500);
  const saved = await js(`(() => { ${helpers} return JSON.stringify(window.__test.updates) + "|" + field("Sustained CPU usage").value + "|" + save().disabled; })()`);
  const [updates, shown, disabled] = saved.split("|");
  const sent = JSON.parse(updates);
  check(
    "saving sends exactly the typed value",
    sent.length === 1 && sent[0].alerts.cpu_high_percent === 85 && shown === "85" && disabled === "true",
    `${sent.length} update(s), cpu_high_percent ${sent[0]?.alerts?.cpu_high_percent}; field ${shown}; nothing left to save ${disabled}`,
  );

  // 3. A daemon that takes 9 s per answer: no request piles up.
  await js(`(() => { Object.assign(window.__test.slow, { get_overview: 9000, get_configuration: 9000, get_metrics: 9000 }); window.__test.max = {}; return true; })()`);
  await sleep(30_000);
  const max = await js(`(() => JSON.stringify(window.__test.max))()`);
  const m = JSON.parse(max);
  check("background requests never overlap", Object.values(m).every((n) => n <= 1) && m.get_overview === 1, `most in flight at once per command over 30 s: ${max}`);
  check("no uncaught exception or console error", consoleErrors.length === 0, consoleErrors.length ? consoleErrors.slice(0, 3).join(" | ") : "console clean for the whole run");
} catch (e) {
  check("browser run", false, String(e));
} finally {
  ws.close();
  ff.kill();
  server.close();
  await sleep(500);
  rmSync(profile, { recursive: true, force: true });
}
console.log(failed ? "SOME CHECKS FAILED" : "ALL CHECKS PASSED");
process.exit(failed ? 1 : 0);
