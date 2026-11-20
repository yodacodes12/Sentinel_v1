// Pure presentation logic of the Schematic Library (Phase 6). Rules, tested:
// - an unverified or related document never looks like an exact match;
// - source trust and hardware match are shown as separate labels;
// - progress is the real stage list, never a percentage;
// - every empty or failure state says what happened and what to do.

import type {
  DocumentCandidate,
  DocumentType,
  DocumentsView,
  MatchClass,
  ProviderState,
  RetrievalStatus,
  RevisionCompatibility,
  SessionRecord,
  SourceTrust,
  Stage,
} from "../types/documents";

export interface ClassStyle {
  label: string;
  /** Tailwind classes: only exact and high-confidence use a solid accent. */
  className: string;
  meaning: string;
}

export const MATCH_CLASS: Record<MatchClass, ClassStyle> = {
  exact: { label: "Exact match", className: "border-green bg-green/15 text-green", meaning: "Model or board and revision match; nothing contradicts them." },
  high_confidence: { label: "High-confidence match", className: "border-green/60 text-green", meaning: "Model or board matches; the revision is not confirmed." },
  related_model: { label: "Related model", className: "border-dashed border-orange/70 text-orange", meaning: "Another model of the family, or another board revision. Not this board; may differ electrically." },
  possible: { label: "Possible match", className: "border-dashed border-line-strong text-muted", meaning: "Some identifiers match; not enough to rely on." },
  unverified: { label: "Unverified", className: "border-dashed border-line text-dim", meaning: "Nothing identifies the hardware it is for." },
  mismatch: { label: "Other hardware", className: "border-red/50 text-red", meaning: "The evidence says it is for different hardware." },
};

export const CLASS_ORDER: MatchClass[] = ["exact", "high_confidence", "related_model", "possible", "unverified", "mismatch"];

export const TRUST: Record<SourceTrust, { label: string; className: string }> = {
  official_this_manufacturer: { label: "Manufacturer site", className: "border-blue/60 text-blue" },
  official_other_manufacturer: { label: "Other manufacturer's site", className: "border-line-strong text-muted" },
  community: { label: "Community upload", className: "border-line-strong text-muted" },
  user_provided: { label: "Your library", className: "border-purple/60 text-purple" },
  unverified: { label: "Unverified source", className: "border-dashed border-line-strong text-dim" },
};

export const DOC_TYPE: Record<DocumentType, string> = {
  schematic: "Schematic",
  boardview: "Boardview",
  service_manual: "Service manual",
  hardware_manual: "Hardware manual",
  specification_sheet: "Specification sheet",
  firmware_documentation: "Firmware documentation",
  unknown: "Unclassified document",
};

export const REVISION: Record<RevisionCompatibility, string> = {
  matches: "Revision matches",
  conflicts: "Different board revision",
  document_unspecified: "Revision not stated by the document",
  device_unknown: "Board revision not reported by firmware",
  not_applicable: "Not established",
};

export const STAGES: { id: Stage; label: string }[] = [
  { id: "preparing_identity", label: "Preparing motherboard identity" },
  { id: "generating_queries", label: "Generating search queries" },
  { id: "searching_providers", label: "Searching configured providers" },
  { id: "collecting_candidates", label: "Collecting document candidates" },
  { id: "identifying_types", label: "Identifying document types" },
  { id: "verifying_matches", label: "Verifying hardware matches" },
  { id: "saving_results", label: "Saving discovery results" },
];

export type StageState = "done" | "current" | "pending";

export function stageStates(stage: Stage | null): { id: Stage; label: string; state: StageState }[] {
  const at = stage ? STAGES.findIndex((s) => s.id === stage) : -1;
  return STAGES.map((s, i) => ({ ...s, state: at < 0 ? "pending" : i < at ? "done" : i === at ? "current" : "pending" }));
}

export const PROVIDER_STATE: Record<ProviderState, string> = {
  ok: "Searched",
  empty: "Searched, nothing found",
  failed: "Failed",
  skipped_offline: "Skipped: online discovery is off",
  not_configured: "Not configured",
  cancelled: "Cancelled",
};

export function retrievalText(r: RetrievalStatus): string {
  switch (r.state) {
    case "not_downloaded":
      return "Not downloaded";
    case "reference_only":
      return "Reference only";
    case "downloaded":
      return "Downloaded copy available";
    case "failed":
      return `Download failed: ${r.reason}`;
    case "in_library":
      return "In your library";
  }
}

/** Downloadable now? Mirrors the daemon's checks so the button is honest. */
export function canDownload(c: DocumentCandidate, online: boolean): { ok: boolean; why: string | null } {
  if (c.retrieval.state === "downloaded") return { ok: false, why: "Already downloaded." };
  if (c.source.access.state === "reference_only") return { ok: false, why: c.source.access.reason };
  if (c.source.access.state === "local_file") return { ok: false, why: "Already on this computer." };
  if (!online) return { ok: false, why: "Turn on online discovery in Settings to download." };
  if (!c.source.document_url) return { ok: false, why: "No document address is known." };
  return { ok: true, why: null };
}

export function canOpenLocal(c: DocumentCandidate): boolean {
  return (c.retrieval.state === "downloaded" || c.retrieval.state === "in_library") && (c.format === "pdf" || c.format === "unknown");
}

export function groups(candidates: DocumentCandidate[]): { cls: MatchClass; items: DocumentCandidate[] }[] {
  return CLASS_ORDER.map((cls) => ({ cls, items: candidates.filter((c) => c.assessment.class === cls) })).filter((g) => g.items.length > 0);
}

export interface EmptyState {
  title: string;
  body: string;
}

/**
 * What the results area says when there is nothing (or nothing reliable)
 * to show. `null` when matching documents exist.
 */
export function emptyState(view: DocumentsView | null, serviceRunning: boolean): EmptyState | null {
  if (!serviceRunning) return { title: "Sentinel service is not running", body: "Start the service to search for documentation. Earlier results are shown again once it runs." };
  if (!view) return { title: "Loading", body: "Reading documentation results from the service." };
  if (!view.identity) return { title: "No motherboard identified", body: view.identity_problem ?? "Run a hardware scan so Sentinel knows which board to look for." };
  if (view.identity.completeness === "insufficient")
    return { title: "Motherboard identity is incomplete", body: "Firmware did not report enough identifiers. Sentinel only searches your document library, because online searches would return unrelated hardware." };
  if (view.progress.running && view.progress.kind === "search") return { title: "Searching", body: "Results appear when the search finishes." };
  const last: SessionRecord | undefined = view.sessions[0];
  if (!last && view.candidates.length === 0) {
    return view.providers.some((p) => p.enabled && p.configured)
      ? { title: "Not searched yet", body: view.online ? "Search to look in your library and the configured online sources." : "Online discovery is off. A search looks only in your document library." }
      : { title: "No providers configured", body: "Turn on online discovery or add documents to your library." };
  }
  if (last?.status === "cancelled" && view.candidates.length === 0) return { title: "Search cancelled", body: "No results were kept from the cancelled search. Search again when ready." };
  if (last?.status === "failed" && view.candidates.length === 0) return { title: "Every provider failed", body: last.error ?? "The sources could not be reached. Check the connection, or search again later." };
  if (view.candidates.length === 0) {
    return view.online
      ? { title: "No documents found", body: "None of the configured sources listed documentation for this board. Not finding a document is not evidence that none exists." }
      : { title: "No documents found offline", body: `Online discovery is off and your library (${view.library_dir}) has no matching documents.` };
  }
  return null;
}

/** A one-line summary above the results ("No exact matches", ...). */
export function resultSummary(view: DocumentsView): string | null {
  if (view.candidates.length === 0) return null;
  const n = (c: MatchClass) => view.candidates.filter((d) => d.assessment.class === c).length;
  if (n("exact") > 0) return `${n("exact")} exact match${n("exact") === 1 ? "" : "es"}. Verify any document before relying on it for repair work.`;
  if (n("high_confidence") > 0) return "No exact matches: the model matches but no document confirms this board revision.";
  if (n("related_model") > 0) return "Only related documents found. They are for other models or revisions and may not match this board electrically.";
  return "No document could be matched to this board.";
}

export function providerFailures(view: DocumentsView): string[] {
  const s = view.sessions[0];
  return s ? s.providers.filter((p) => p.state === "failed").map((p) => `${p.provider_name}: ${p.error ?? "failed"}`) : [];
}

export function formatTime(unixSeconds: number | null): string {
  if (!unixSeconds) return "Never";
  return new Date(unixSeconds * 1000).toLocaleString([], { year: "numeric", month: "short", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false });
}
