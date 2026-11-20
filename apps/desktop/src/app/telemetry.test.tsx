// Phase 3 frontend tests: live telemetry display, charts, alerts.
// All values are synthetic.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { Chart } from "../components/Chart";
import { AlertCard, AlertsPage } from "../pages/AlertsPage";
import { TelemetryPage } from "../pages/telemetry/TelemetryPage";
import { ProcessTable, sortProcesses } from "../pages/telemetry/TelemetryTabs";
import type { Alert, MonitoringReport, ProcessSample, Reading, TelemetrySnapshot } from "../types/telemetry";
import { linePath, niceMax, phaseLabel, reading, runtimeText } from "./telemetryView";

const ok = <T,>(value: T, quality: Reading<T>["quality"] = "direct"): Reading<T> => ({ value, state: "ok", quality });
const no = <T,>(state: Reading<T>["state"]): Reading<T> => ({ state, quality: "direct" });
const text = (html: string) => html.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ");

function report(phase: MonitoringReport["phase"]): MonitoringReport {
  return {
    phase,
    since: 0,
    sampling: { selected: "balanced", effective: "balanced", power_aware: false, reason: null, intervals_ms: [["cpu", 2000]] },
    collectors: [],
    last_sample_at: phase === "active" || phase === "paused" ? 1_794_900_000_000 : null,
    error: null,
    enabled_preference: phase !== "off",
    history_available: true,
    notifications: { enabled: true, last_delivery_ok: null, last_error: null, delivered: 0, failed: 0 },
  };
}

const proc = (pid: number, name: string, cpu: Reading<number>, rss: number, state: ProcessSample["state"] = "sleeping"): ProcessSample => ({
  pid,
  name,
  command: name,
  state,
  uid: 1000,
  user: "example",
  cpu_percent: cpu,
  rss_bytes: rss,
  virtual_bytes: rss * 4,
  memory_percent: ok(1.5),
  threads: 3,
  started_at: 1_794_000_000,
});

function snapshot(): TelemetrySnapshot {
  const p = (v: number) => ok(v, "calculated");
  return {
    cpu: {
      measured_at: 1,
      interval_ms: 2000,
      logical_cpus: 2,
      total_percent: p(42.5),
      times: {
        user_percent: p(30),
        nice_percent: p(0),
        system_percent: p(10),
        idle_percent: p(57.5),
        iowait_percent: p(2.5),
        irq_percent: p(0),
        softirq_percent: p(0),
        steal_percent: p(0),
      },
      cores: [
        { cpu: 0, utilisation_percent: p(80), frequency_khz: ok(3_000_000) },
        { cpu: 1, utilisation_percent: no("pending"), frequency_khz: no("unsupported") },
      ],
      average_frequency_khz: p(3_000_000),
    },
    memory: null,
    battery: {
      measured_at: 1,
      interval_ms: 15000,
      on_ac: ok(false),
      sources: [],
      batteries: [
        {
          measured_at: 1,
          interval_ms: 15000,
          name: "BAT0",
          state: ok("discharging"),
          percent: ok(55),
          energy_now_wh: ok(25),
          energy_full_wh: ok(45),
          energy_design_wh: ok(50),
          charge_now_ah: no("unsupported"),
          charge_full_ah: no("unsupported"),
          charge_design_ah: no("unsupported"),
          voltage_v: ok(11.5),
          current_a: no("unsupported"),
          power_w: ok(9.3),
          cycle_count: no("unavailable"),
          health_percent: ok(90, "estimated"),
          temperature_c: no("unsupported"),
          runtime: { state: "calculating", samples: 2, needed: 4 },
          technology: null,
          manufacturer: null,
          model: null,
        },
      ],
    },
    thermal: null,
    fans: null,
    gpu: null,
    storage: null,
    network: null,
    load: null,
    health: [],
    top_processes: [],
  };
}

describe("reading display", () => {
  it("explains missing values and labels estimates", () => {
    expect(reading(no<number>("pending")).text).toBe("Measuring…");
    expect(reading(no<number>("permission_denied")).text).toBe("Administrator only");
    expect(reading(no<number>("unsupported")).present).toBe(false);
    expect(reading(ok(0)).text).toBe("0");
    expect(reading(ok(90, "estimated")).estimated).toBe(true);
  });

  it("never presents a runtime countdown without enough data", () => {
    expect(runtimeText({ state: "calculating", samples: 1, needed: 4 }).text).toBe("Calculating remaining time...");
    expect(runtimeText({ state: "insufficient", reason: "x" }).text).toBe("Insufficient battery data for a reliable estimate.");
    const est = runtimeText({ state: "estimated", seconds: 3 * 3600 + 7 * 60 + 13, confidence: "medium", average_power_w: 9.3, samples: 8, window_s: 240 });
    expect(est.text).toBe("About 3 h 05 min (estimate)");
    expect(est.detail).toContain("not a countdown");
    expect(est.detail).toContain("average draw 9.3 W over 4 min");
    // A draw that could not be expressed in watts is left out, not invented.
    const noDraw = runtimeText({ state: "estimated", seconds: 3600, confidence: "high", average_power_w: null, samples: 8, window_s: 240 });
    expect(noDraw.detail).toBe("high confidence · not a countdown");
    // Low confidence rounds to 15 minutes.
    expect(runtimeText({ state: "estimated", seconds: 3 * 3600 + 7 * 60, confidence: "low", average_power_w: 9, samples: 4, window_s: 60 }).text).toBe("About 3 h 00 min (estimate)");
  });

  it("is only live when monitoring is active", () => {
    expect(phaseLabel("active").live).toBe(true);
    for (const p of ["off", "paused", "starting", "stopping", "error", null] as const) expect(phaseLabel(p).live).toBe(false);
  });
});

describe("charts", () => {
  const g = { width: 100, height: 10, tMin: 0, tMax: 100, yMin: 0, yMax: 100 };

  it("break lines at gaps and never bridge missing data", () => {
    const d = linePath(
      [
        { t: 0, v: 10 },
        { t: 10, v: 20 },
        { t: 20, v: null },
        { t: 30, v: 30 },
        { t: 90, v: 40 },
      ],
      g,
      25,
    );
    expect(d.match(/M/g)?.length).toBe(3); // [0,10] | [30] | [90] (60 ms jump > 25)
    expect(linePath([{ t: 0, v: null }], g, 10)).toBe("");
  });

  it("scales sensibly", () => {
    expect(niceMax([[{ t: 0, v: 3 }]], "%")).toBe(100);
    expect(niceMax([[{ t: 0, v: 3_400 }]], "B/s")).toBe(5_000);
    expect(niceMax([[{ t: 0, v: null }]], "W")).toBe(1);
  });

  it("render an empty state rather than a fake line", () => {
    const html = renderToStaticMarkup(<Chart series={[{ key: "cpu.total", unit: "%", source: "/proc/stat", quality: "calculated", resolution: "raw", points: [{ t: 1, v: null }] }]} window="minutes5" />);
    expect(html).toContain("No data");
    expect(html).not.toContain("<path");
  });
});

describe("telemetry page", () => {
  const render = (r: MonitoringReport | null, s: TelemetrySnapshot | null, tab?: "cpu" | "battery") =>
    renderToStaticMarkup(<TelemetryPage report={r} snapshot={s} processor={null} thresholds={null} serviceRunning onStart={() => {}} initialTab={tab} />);

  it("shows no measurements while monitoring is off", () => {
    const html = text(render(report("off"), null));
    expect(html).toContain("Monitoring is off");
    expect(html).toContain("Turn monitoring on");
    expect(html).not.toMatch(/\d+(\.\d+)?\s?%/);
  });

  it("shows live values, with pending readings explained", () => {
    const html = text(render(report("active"), snapshot()));
    expect(html).toContain("42.5%");
    expect(html).toContain("Measuring…");
    expect(html).toContain("Live · last update");
  });

  it("never calls data live when the daemon stopped answering", () => {
    const html = text(renderToStaticMarkup(<TelemetryPage report={report("active")} snapshot={snapshot()} processor={null} thresholds={null} serviceRunning stale onStart={() => {}} />));
    expect(html).toContain("Not live · Sentinel is not answering");
    expect(html).not.toContain("Live · last update");
  });

  it("says so when history cannot be recorded", () => {
    const r = { ...report("active"), history_available: false };
    expect(text(render(r, snapshot()))).toContain("History is unavailable");
    expect(text(render(report("active"), snapshot()))).not.toContain("History is unavailable");
  });

  it("labels paused data as not live", () => {
    const html = text(render(report("paused"), snapshot()));
    expect(html).toContain("Monitoring is paused");
    expect(html).toContain("Not live");
  });

  it("battery: estimate labels, no invented temperature or runtime", () => {
    const html = text(render(report("active"), snapshot(), "battery"));
    expect(html).toContain("Calculating remaining time...");
    expect(html).toContain("estimate");
    expect(html).toMatch(/Temperature Not provided/);
    expect(html).not.toMatch(/\d+ h \d+ min/);
  });
});

describe("processes", () => {
  const list = [proc(1, "idle", ok(0), 10), proc(2, "busy", ok(90), 20, "running"), proc(3, "fat", no("pending"), 900)];

  it("sort, search and filter", () => {
    expect(sortProcesses(list, "cpu", "", "all").map((p) => p.pid)).toEqual([2, 1, 3]);
    expect(sortProcesses(list, "memory", "", "all").map((p) => p.pid)).toEqual([3, 2, 1]);
    expect(sortProcesses(list, "cpu", "fa", "all").map((p) => p.pid)).toEqual([3]);
    expect(sortProcesses(list, "cpu", "", "running").map((p) => p.pid)).toEqual([2]);
  });

  it("offer no controls over processes", () => {
    const html = renderToStaticMarkup(<ProcessTable processes={list} />);
    expect(html).not.toMatch(/<button/);
    for (const word of ["Kill", "Terminate", "End task", "Suspend", "Priority", "Renice"]) expect(html).not.toContain(word);
  });
});

describe("alerts", () => {
  const alert: Alert = {
    id: 3,
    rule: "memory_low",
    category: "memory",
    severity: "warning",
    title: "Low available memory",
    explanation: "Available memory has remained below 10% of total.",
    recommendation: "Review the applications consuming the most memory.",
    metric: "mem.available",
    subject: null,
    threshold: "≤ 10% available",
    evidence: "7.5% of memory available.",
    detected_at: 1_794_900_000_000,
    last_updated_at: 1_794_900_000_000,
    resolved_at: null,
    status: "active",
    acknowledged: false,
    notified: true,
    threshold_source: "default",
  };

  it("show evidence, threshold and a text-only recommendation", () => {
    const html = renderToStaticMarkup(<AlertCard alert={alert} onAcknowledge={() => {}} />);
    const t = text(html);
    expect(t).toContain("7.5% of memory available.");
    expect(t).toContain("≤ 10% available (default)");
    expect(t).toContain("You may consider Review the applications");
    // The only button is Acknowledge.
    expect(html.match(/<button/g)?.length).toBe(1);
    expect(t).toContain("Acknowledge");
    expect(html).not.toMatch(/Fix|Repair|Run|Apply|Close app/);
  });

  it("acknowledged alerts have no button at all", () => {
    expect(renderToStaticMarkup(<AlertCard alert={{ ...alert, acknowledged: true }} />)).not.toMatch(/<button/);
  });

  it("page lists active alerts", () => {
    const t = text(renderToStaticMarkup(<AlertsPage active={[alert]} onAcknowledge={() => {}} serviceRunning />));
    expect(t).toContain("Active alerts · 1");
    expect(t).toContain("Alerts inform; they never act.");
  });
});

describe("sources", () => {
  const files = (dir: string): string[] =>
    readdirSync(dir).flatMap((name) => {
      const path = join(dir, name);
      return statSync(path).isDirectory() ? files(path) : [path];
    });

  it("contain no process control, system changes or direct file access", () => {
    const sources = files(join(import.meta.dirname, "..")).filter((p) => /\.(ts|tsx)$/.test(p) && !p.includes(".test."));
    for (const path of sources) {
      const code = readFileSync(path, "utf8");
      for (const word of ["kill_process", "\"kill\"", "terminate_process", "renice", "set_priority", "set_fan", "cpufreq/scaling_governor", "node:fs", "readFileSync", "plugin-fs", "plugin-shell"]) {
        expect(code.includes(word), `${path} contains ${word}`).toBe(false);
      }
    }
  });
});
