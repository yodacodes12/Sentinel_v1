// Frontend safety tests: the UI must never present placeholder content as
// real, and must never report monitoring as active when it is not.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { OverviewPage } from "../pages/OverviewPage";
import { PlaceholderPage } from "../pages/PlaceholderPage";
import type { DaemonStatus, Overview } from "../types/sentinel";
import { NAV_ITEMS } from "./catalog";
import { formatUptime, monitoringIndicator, serviceIndicator } from "./viewModel";

function daemon(overrides: Partial<DaemonStatus> = {}): DaemonStatus {
  return {
    app_version: "0.1.0",
    protocol_version: 1,
    pid: 4242,
    state: "running",
    started_at: 1_794_846_321,
    uptime_secs: 133,
    monitoring: { state: "stopped", engine_available: false, note: "No engine in this release." },
    operating_mode: "balanced",
    config_path: "/home/u/.config/sentinel/config.toml",
    storage_available: true,
    recovered_from_unclean_shutdown: false,
    ...overrides,
  };
}

function overview(status: DaemonStatus | null): Overview {
  return {
    client_version: "0.1.0",
    running: status !== null,
    ipc: status ? "connected" : "not_running",
    ipc_detail: null,
    socket_path: "/run/user/1000/sentinel/daemon.sock",
    daemon: status,
    operating_mode: "balanced",
    config_path: "/home/u/.config/sentinel/config.toml",
    log_path: "/home/u/.local/state/sentinel/logs/daemon.log",
  };
}

describe("monitoring indicator", () => {
  it("is never active without an engine, whatever state is claimed", () => {
    for (const state of ["stopped", "running", "paused"] as const) {
      const indicator = monitoringIndicator(
        "ready",
        overview(daemon({ monitoring: { state, engine_available: false, note: "" } })),
      );
      expect(indicator.label).toBe("Not available");
      expect(indicator.tone).not.toBe("ok");
    }
  });

  it("is inactive when the service is stopped, loading or unreachable", () => {
    expect(monitoringIndicator("ready", overview(null)).label).toBe("Inactive");
    expect(monitoringIndicator("loading", null).label).toBe("Inactive");
    // Stale data from before the bridge was lost must not be shown as live.
    const stale = overview(daemon({ monitoring: { state: "running", engine_available: true, note: "" } }));
    expect(monitoringIndicator("unavailable", stale).label).toBe("Inactive");
  });

  it("is unknown, never active or inactive, when the daemon does not answer", () => {
    const hung = { ...overview(null), ipc: "error" as const, ipc_detail: "timed out waiting for the daemon's answer" };
    expect(monitoringIndicator("ready", hung)).toMatchObject({ label: "Unknown", tone: "unknown" });
    expect(serviceIndicator("ready", hung)).toMatchObject({ label: "Unreachable", tone: "error" });
  });

  it("reports active only for a running engine", () => {
    const running = overview(daemon({ monitoring: { state: "running", engine_available: true, note: "" } }));
    expect(monitoringIndicator("ready", running)).toMatchObject({ label: "Active", tone: "ok" });
    const paused = overview(daemon({ monitoring: { state: "paused", engine_available: true, note: "" } }));
    expect(monitoringIndicator("ready", paused).label).toBe("Paused");
  });
});

describe("service indicator", () => {
  it("reflects the daemon's reported state", () => {
    expect(serviceIndicator("ready", overview(daemon())).label).toBe("Running");
    expect(serviceIndicator("ready", overview(null)).label).toBe("Stopped");
    expect(serviceIndicator("loading", null).tone).toBe("unknown");
    expect(serviceIndicator("unavailable", null).label).toBe("Unknown");
    expect(serviceIndicator("ready", overview(daemon({ state: "shutting_down" }))).tone).toBe("warn");
    const broken: Overview = { ...overview(null), ipc: "error", ipc_detail: "socket permission denied" };
    expect(serviceIndicator("ready", broken)).toMatchObject({ tone: "error", detail: "socket permission denied" });
  });
});

describe("catalog", () => {
  it("lists the approved navigation, with only built sections marked implemented", () => {
    expect(NAV_ITEMS.map((item) => item.label)).toEqual([
      "Overview",
      "Hardware",
      "Motherboard",
      "Telemetry",
      "Schematic Library",
      "Diagnostics",
      "Installed Utilities",
      "Services and Startup",
      "Alerts and Events",
      "Settings",
    ]);
    expect(NAV_ITEMS.filter((item) => item.implemented).map((item) => item.id)).toEqual([
      "overview",
      "hardware",
      "motherboard",
      "telemetry",
      // Phase 6: document discovery and verification.
      "schematics",
      "utilities",
      "services",
      "alerts",
      "settings",
    ]);
  });
});

describe("rendered dashboard", () => {
  const render = (status: DaemonStatus | null) =>
    renderToStaticMarkup(
      <OverviewPage
        phase="ready"
        overview={overview(status)}
        config={null}
        saving={false}
        onPowerMode={() => {}}
        inventory={null}
        progress={null}
        onScan={() => {}}
        onCancel={() => {}}
        onOpenHardware={() => {}}
      />,
    );

  it("shows nothing as detected before a scan, and no readings", () => {
    const html = render(daemon());
    expect(html).toContain("Not scanned");
    expect(html.match(/Run a hardware scan/g)?.length).toBeGreaterThanOrEqual(6);
    expect(html).not.toContain("<svg viewBox=\"0 0 600 400\"");
    // Without telemetry, no live value is shown anywhere.
    expect(html).toContain("Monitoring off · no live data");
    // No units of measurement anywhere: nothing on the page is a reading.
    const text = html.replace(/<[^>]*>/g, " ");
    expect(text).not.toMatch(/\d\s?(°C|%|GB|MB|MHz|GHz|RPM|W\b|V\b)/);
  });

  it("shows the daemon's real identity when running", () => {
    const html = render(daemon());
    expect(html).toContain("pid 4242");
    expect(html).toContain("protocol v1");
    expect(html).toContain("up 2m 13s at last refresh");
    expect(html).toContain("Off");
  });

  it("shows a stopped service without stale details", () => {
    const html = render(null);
    expect(html).toContain("Stopped");
    expect(html).toContain("Not connected");
    expect(html).not.toContain("pid ");
    expect(html).toContain("Monitoring off");
  });

  it("placeholder sections show no data", () => {
    for (const item of NAV_ITEMS.filter((candidate) => !candidate.implemented)) {
      const html = renderToStaticMarkup(<PlaceholderPage item={item} />);
      expect(html).toContain("In development");
      expect(html).toContain("Nothing is collected or displayed here yet");
    }
  });
});

describe("formatting", () => {
  it("formats uptime", () => {
    expect(formatUptime(5)).toBe("5s");
    expect(formatUptime(133)).toBe("2m 13s");
    expect(formatUptime(3_700)).toBe("1h 1m");
    expect(formatUptime(90_061)).toBe("1d 1h 1m");
  });
});

describe("frontend sources", () => {
  const files = (dir: string): string[] =>
    readdirSync(dir).flatMap((name) => {
      const path = join(dir, name);
      return statSync(path).isDirectory() ? files(path) : [path];
    });
  const sources = files(join(import.meta.dirname, "..")).filter(
    (path) => /\.(ts|tsx)$/.test(path) && !path.endsWith(".test.tsx"),
  );

  it("make no network requests and run no continuous animation", () => {
    expect(sources.length).toBeGreaterThan(10);
    for (const path of sources) {
      const code = readFileSync(path, "utf8");
      for (const forbidden of ["fetch(", "XMLHttpRequest", "WebSocket", "EventSource", "sendBeacon", "http://", "https://"]) {
        expect(code.includes(forbidden), `${path} contains ${forbidden}`).toBe(false);
      }
      for (const forbidden of ["animate-", "requestAnimationFrame", "@keyframes"]) {
        expect(code.includes(forbidden), `${path} contains ${forbidden}`).toBe(false);
      }
    }
  });

  it("only the bridge talks to the backend", () => {
    for (const path of sources) {
      if (path.endsWith(join("services", "bridge.ts"))) continue;
      expect(readFileSync(path, "utf8").includes("@tauri-apps/"), `${path} bypasses the bridge`).toBe(false);
    }
  });
});
