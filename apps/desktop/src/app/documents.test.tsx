// Schematic Library tests. `test-fixtures/documents-view.json` is produced by
// the real discovery engine and matcher (crates/sentinel-docs, engine.rs),
// so these tests check the Rust model as the frontend receives it.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { HardwarePage } from "../pages/HardwarePage";
import { SchematicLibraryPage, type DocumentsControls } from "../pages/SchematicLibraryPage";
import { DOCUMENT_DEFAULTS, catalogueLines } from "../pages/SettingsPage";
import type { DocumentsView } from "../types/documents";
import type { InventoryView, JobProgress, SystemInventory, ComponentIndex } from "../types/inventory";
import { MATCH_CLASS, STAGES, TRUST, canDownload, emptyState, groups, resultSummary, stageStates } from "./documentsView";

const load = (): DocumentsView => JSON.parse(readFileSync(join(import.meta.dirname, "../test-fixtures/documents-view.json"), "utf8"));
const text = (html: string) => html.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ");
const noop = () => {};
const controls = (view: DocumentsView | null, extra: Partial<DocumentsControls> = {}): DocumentsControls => ({
  view,
  busy: false,
  search: noop,
  cancel: noop,
  download: noop,
  removeFile: noop,
  clearCache: noop,
  open: noop,
  ...extra,
});
const render = (view: DocumentsView | null, selected: number | null = null, running = true) =>
  renderToStaticMarkup(<SchematicLibraryPage controls={controls(view)} serviceRunning={running} onOpenSettings={noop} initialSelected={selected} />);

describe("motherboard overview", () => {
  it("shows the detected identity, its reliability and the last discovery", () => {
    const html = text(render(load()));
    for (const s of ["Dell Inc.", "Inspiron 15 3535", "0N90HF", "A02", "firmware-reported, not externally verified", "Online discovery on"]) expect(html).toContain(s);
  });

  it("says when identification is incomplete", () => {
    const v = load();
    v.identity!.completeness = "insufficient";
    v.identity!.board_revision = null;
    const html = text(render(v));
    expect(html).toContain("Incomplete: online search disabled");
    expect(html).toContain("Not reported by firmware");
  });
});

describe("results", () => {
  it("groups by match class and never makes unverified look like exact", () => {
    const v = load();
    expect(groups(v.candidates).map((g) => g.cls)).toEqual(["exact", "high_confidence", "related_model", "possible", "unverified", "mismatch"]);
    expect(MATCH_CLASS.exact.className).not.toEqual(MATCH_CLASS.unverified.className);
    expect(MATCH_CLASS.unverified.className).toContain("dashed");
    expect(MATCH_CLASS.related_model.className).toContain("dashed");
    const html = text(render(v));
    for (const s of ["Exact match · 1", "High-confidence match · 2", "Related model · 2", "Possible match · 1", "Unverified · 1"]) expect(html).toContain(s);
    // Other hardware is folded away by default.
    expect(html).not.toContain("Lenovo ThinkPad T480");
    expect(html).toContain("Show 1 document(s) for other hardware");
  });

  it("shows source trust separately from the hardware match", () => {
    const v = load();
    const html = text(render(v));
    expect(html).toContain(TRUST.user_provided.label);
    expect(html).toContain(TRUST.official_this_manufacturer.label);
    expect(html).toContain(TRUST.community.label);
    // An official source is never a match class by itself.
    const official = v.candidates.find((c) => c.source.trust === "official_this_manufacturer")!;
    expect(official.assessment.class).toBe("high_confidence");
  });

  it("summarises what was and was not found", () => {
    const v = load();
    expect(resultSummary(v)).toContain("1 exact match");
    v.candidates = v.candidates.filter((c) => c.assessment.class !== "exact");
    expect(resultSummary(v)).toContain("No exact matches");
    v.candidates = v.candidates.filter((c) => !["high_confidence"].includes(c.assessment.class));
    expect(resultSummary(v)).toContain("Only related documents");
  });

  it("reports a provider failure without hiding other results", () => {
    const html = text(render(load()));
    expect(html).toContain("Provider unavailable: broken");
    expect(html).toContain("Results from other sources are shown");
  });
});

describe("detail panel", () => {
  it("explains matching, contradicting and missing evidence", () => {
    const v = load();
    const related = v.candidates.find((c) => c.title.includes("rev A00"))!;
    const html = text(render(v, related.id));
    expect(html).toContain("Different board revision");
    expect(html).toContain("What matched");
    expect(html).toContain("What contradicts it");
    expect(html).toContain("States board revision A00, not A02.");
    expect(html).toContain("Manual verification recommended");
    expect(html).toContain("Source (separate from the hardware match)");
  });

  it("offers downloads only where the source permits them", () => {
    const v = load();
    const official = v.candidates.find((c) => c.source.trust === "official_this_manufacturer")!;
    const community = v.candidates.find((c) => c.source.trust === "community")!;
    const library = v.candidates.find((c) => c.source.trust === "user_provided")!;
    expect(canDownload(official, true).ok).toBe(true);
    expect(canDownload(official, false)).toEqual({ ok: false, why: "Turn on online discovery in Settings to download." });
    expect(canDownload(community, true).ok).toBe(false);
    expect(canDownload(library, true).ok).toBe(false);
    const html = text(render(v, community.id));
    expect(html).toContain("Download unavailable: Community upload with unverified rights");
    expect(html).toContain("Open source page");
    expect(text(render(v, library.id))).toContain("Open local copy");
    expect(text(render(v, official.id))).toContain("(from extension, not verified)");
  });
});

describe("progress and cancellation", () => {
  it("shows the real stages and a cancel control, never a percentage", () => {
    const v = load();
    v.progress = { running: true, kind: "search", stage: "verifying_matches", started_at: 1, session_id: 2, document_id: null, cancel_requested: false };
    const states = stageStates("verifying_matches");
    expect(states.map((s) => s.state)).toEqual(["done", "done", "done", "done", "done", "current", "pending"]);
    const html = text(render(v));
    for (const s of STAGES) expect(html).toContain(s.label);
    expect(html).toContain("Cancel");
    expect(html).not.toMatch(/\d+\s?%/);
    v.progress.cancel_requested = true;
    expect(text(render(v))).toContain("Cancelling");
  });
});

describe("empty and failure states", () => {
  const base = () => {
    const v = load();
    v.candidates = [];
    return v;
  };
  it("covers every state the plan lists", () => {
    expect(emptyState(null, false)!.title).toBe("Sentinel service is not running");
    const noBoard = base();
    noBoard.identity = null;
    noBoard.identity_problem = "Run a hardware scan first: discovery needs the motherboard identity.";
    expect(emptyState(noBoard, true)!.title).toBe("No motherboard identified");
    const incomplete = base();
    incomplete.identity!.completeness = "insufficient";
    expect(emptyState(incomplete, true)!.title).toBe("Motherboard identity is incomplete");
    const fresh = base();
    fresh.sessions = [];
    expect(emptyState(fresh, true)!.title).toBe("Not searched yet");
    fresh.providers = fresh.providers.map((p) => ({ ...p, enabled: false }));
    expect(emptyState(fresh, true)!.title).toBe("No providers configured");
    const cancelled = base();
    cancelled.sessions[0]!.status = "cancelled";
    expect(emptyState(cancelled, true)!.title).toBe("Search cancelled");
    const failed = base();
    failed.sessions[0]!.status = "failed";
    expect(emptyState(failed, true)!.title).toBe("Every provider failed");
    const none = base();
    expect(emptyState(none, true)!.title).toBe("No documents found");
    none.online = false;
    expect(emptyState(none, true)!.title).toBe("No documents found offline");
    expect(emptyState(load(), true)).toBeNull();
  });

  it("never shows placeholder documents", () => {
    const html = text(render(base()));
    expect(html).toContain("No documents found");
    expect(html).not.toContain("Exact match ·");
  });

  it("explains offline mode", () => {
    const v = load();
    v.online = false;
    const html = text(render(v));
    expect(html).toContain("Offline: nothing is sent anywhere. Only your document library is searched.");
  });
});

describe("integration with the explorer and settings", () => {
  it("summarises documentation in the explorer's motherboard category", () => {
    const data = JSON.parse(readFileSync(join(import.meta.dirname, "../test-fixtures/explorer-amd-laptop.json"), "utf8")) as { inventory: SystemInventory; components: ComponentIndex };
    const progress: JobProgress = { job: "hardware", state: "completed", started_at: 1, finished_at: 2, current_module: null, modules: [], last_successful_scan: 1, error: null, cache_saved: true };
    const view: InventoryView = { inventory: data.inventory, components: data.components, origin: "this_session", source: "daemon", progress, warning: null };
    const html = text(renderToStaticMarkup(<HardwarePage inventory={view} progress={progress} serviceRunning onScan={noop} onCancel={noop} category="motherboard" onCategory={noop} selection={{}} onSelection={noop} documents={load()} onOpenDocuments={noop} />));
    expect(html).toContain("Board documentation");
    expect(html).toContain("1 exact, 2 high-confidence and 2 related");
    expect(html).toContain("Open the Schematic Library");
  });

  it("keeps online discovery and automatic downloads off by default", () => {
    expect(DOCUMENT_DEFAULTS.online_discovery).toBe(false);
    expect(DOCUMENT_DEFAULTS.auto_download).toBe(false);
    expect(catalogueLines(" https://a.example.org/x.json \n\n https://b.example.org/y.json")).toEqual(["https://a.example.org/x.json", "https://b.example.org/y.json"]);
  });
});
