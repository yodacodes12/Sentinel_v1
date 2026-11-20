import { useState } from "react";

import {
  DOC_TYPE,
  MATCH_CLASS,
  PROVIDER_STATE,
  REVISION,
  TRUST,
  canDownload,
  canOpenLocal,
  emptyState,
  formatTime,
  groups,
  providerFailures,
  resultSummary,
  retrievalText,
  stageStates,
} from "../app/documentsView";
import { formatBytes } from "../app/inventoryView";
import { EmptyState, Panel } from "../components/primitives";
import type { DocumentCandidate, DocumentsView, Evidence, OpenTarget } from "../types/documents";

export interface DocumentsControls {
  view: DocumentsView | null;
  busy: boolean;
  search: (force: boolean) => void;
  cancel: () => void;
  download: (id: number) => void;
  removeFile: (id: number) => void;
  clearCache: () => void;
  open: (id: number, target: OpenTarget) => void;
}

function Badge({ className, children, title }: { className: string; children: React.ReactNode; title?: string }) {
  return (
    <span title={title} className={`inline-block shrink-0 border px-1.5 py-px font-mono text-[9.5px] uppercase tracking-wider ${className}`}>
      {children}
    </span>
  );
}

function Button({ children, onClick, disabled, title, primary = false }: { children: React.ReactNode; onClick: () => void; disabled?: boolean; title?: string; primary?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      className={`border px-2.5 py-1 text-xs disabled:cursor-not-allowed disabled:opacity-40 ${primary ? "border-cyan/70 text-cyan hover:bg-cyan/10" : "border-line-strong text-muted hover:bg-raised hover:text-fg"}`}
    >
      {children}
    </button>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    // A paragraph, not a div: WebKitGTK exposes paragraph text to screen
    // readers, but not spans inside a plain grid div.
    <p className="grid grid-cols-[120px_1fr] gap-2 border-b border-line py-1 text-[11px] last:border-b-0">
      <span className="text-muted">{label}</span>
      <span className="selectable min-w-0 break-words font-mono text-fg">{children}</span>
    </p>
  );
}

function Overview({ view }: { view: DocumentsView | null }) {
  const id = view?.identity;
  const v = (x: { original: string } | null | undefined, missing: string) => (x ? x.original : <span className="text-dim">{missing}</span>);
  return (
    <Panel title="Motherboard documentation" aside={<span className="label">{view?.online ? "Online discovery on" : "Offline: library only"}</span>}>
      {id ? (
        <>
          <Row label="Manufacturer">{v(id.manufacturer, "Not reported")}</Row>
          <Row label="System model">{v(id.system_model, "Not reported")}</Row>
          <Row label="Board identifier">{v(id.board_name, "Not reported")}</Row>
          <Row label="Board revision">{v(id.board_revision, "Not reported by firmware")}</Row>
          <Row label="Identification">
            {id.completeness === "full" ? "Manufacturer, model and board identifier (firmware-reported, not externally verified)" : id.completeness === "insufficient" ? "Incomplete: online search disabled" : `Partial (${id.completeness.replace("_", " ")})`}
          </Row>
          <Row label="Last discovery">{formatTime(view?.last_search_at ?? null)}</Row>
          {id.notes.length ? <p className="pt-2 text-[10px] text-dim">{id.notes.join(" ")}</p> : null}
        </>
      ) : (
        <p className="text-xs text-dim">{view?.identity_problem ?? "No motherboard identity yet."}</p>
      )}
    </Panel>
  );
}

function Progress({ view }: { view: DocumentsView }) {
  if (!view.progress.running) return null;
  if (view.progress.kind === "download") {
    return (
      <Panel title="Downloading">
        <p role="status" className="text-xs text-muted">
          Downloading document {view.progress.document_id}. The file is checked before it is kept.
        </p>
      </Panel>
    );
  }
  return (
    <Panel title="Search progress" aside={view.progress.cancel_requested ? <span className="label text-orange">Cancelling</span> : undefined}>
      <ol role="status" aria-label="Search stages">
        {stageStates(view.progress.stage).map((s) => (
          <li key={s.id} className={`flex items-center gap-2 py-0.5 text-[11px] ${s.state === "current" ? "text-cyan" : s.state === "done" ? "text-muted" : "text-dim"}`}>
            <span className="w-3 font-mono">{s.state === "done" ? "✓" : s.state === "current" ? "›" : "·"}</span>
            {s.label}
          </li>
        ))}
      </ol>
    </Panel>
  );
}

function Providers({ view }: { view: DocumentsView }) {
  const last = view.sessions[0];
  return (
    <Panel title="Sources">
      <ul>
        {view.providers.map((p) => {
          const outcome = last?.providers.find((o) => o.provider_id === p.id);
          return (
            <li key={p.id} className="border-b border-line py-1 text-[11px] last:border-b-0">
              <div className="flex justify-between">
                <span className={p.enabled ? "text-fg" : "text-dim"}>{p.name}</span>
                <span className="font-mono text-[10px] text-dim">{!p.enabled ? "Off (online discovery off)" : !p.configured ? "Not configured" : p.online ? "Online" : "Offline"}</span>
              </div>
              <div className="text-[10px] text-dim">{p.description}</div>
              {outcome ? (
                <div className={`text-[10px] ${outcome.state === "failed" ? "text-orange" : "text-muted"}`}>
                  Last search: {PROVIDER_STATE[outcome.state]}
                  {outcome.results ? ` · ${outcome.results} result${outcome.results === 1 ? "" : "s"}` : ""}
                  {outcome.error ? ` · ${outcome.error}` : ""}
                </div>
              ) : null}
            </li>
          );
        })}
      </ul>
      {view.sessions.length ? (
        <details className="mt-2">
          <summary className="label cursor-pointer text-muted">Previous discovery sessions ({view.sessions.length})</summary>
          <ul className="mt-1">
            {view.sessions.map((s) => (
              <li key={s.id} className="font-mono text-[10px] text-dim">
                {formatTime(s.started_at)} · {s.status} · {s.candidates} result{s.candidates === 1 ? "" : "s"} · {s.online ? "online" : "offline"}
                {s.queries.length ? ` · ${s.queries.length} queries` : ""}
              </li>
            ))}
          </ul>
        </details>
      ) : null}
      <p className="mt-2 text-[10px] text-dim">
        Your library: <span className="selectable font-mono">{view.library_dir}</span>. Put documents you already have there; Sentinel reads them and never changes them.
      </p>
    </Panel>
  );
}

function ResultItem({ c, selected, onSelect }: { c: DocumentCandidate; selected: boolean; onSelect: () => void }) {
  const cls = MATCH_CLASS[c.assessment.class];
  const trust = TRUST[c.source.trust];
  return (
    <li>
      <button
        type="button"
        onClick={onSelect}
        aria-pressed={selected}
        aria-label={`${c.title}, ${cls.label}, ${trust.label}`}
        className={`w-full border-l-2 px-2 py-1.5 text-left ${selected ? "border-cyan bg-cyan/10" : "border-transparent hover:bg-raised"}`}
      >
        <div className="flex items-start gap-2">
          <span className="min-w-0 flex-1 truncate text-xs text-fg">{c.title}</span>
          <Badge className={cls.className} title={cls.meaning}>
            {cls.label}
          </Badge>
        </div>
        <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[10px] text-dim">
          <span>{DOC_TYPE[c.doc_type]}</span>·<span>{c.source.source_name}</span>
          <Badge className={trust.className}>{trust.label}</Badge>
          <span>· {REVISION[c.assessment.revision]}</span>
          <span>· {retrievalText(c.retrieval)}</span>
        </div>
      </button>
    </li>
  );
}

function EvidenceList({ title, items, tone }: { title: string; items: Evidence[]; tone: string }) {
  if (items.length === 0) return null;
  return (
    <div className="mt-2">
      <span className="label">{title}</span>
      <ul className="mt-0.5">
        {items.map((e, i) => (
          <li key={i} className={`text-[11px] ${tone}`}>
            {e.note}
            <span className="ml-1 font-mono text-[10px] text-dim">
              ({e.location.replace("_", " ")}
              {e.weight ? `, ${e.effect === "contradicts" ? "−" : "+"}${e.weight}` : ""})
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function Detail({ c, view, controls }: { c: DocumentCandidate | null; view: DocumentsView; controls: DocumentsControls }) {
  if (!c) {
    return (
      <Panel title="Document details" className="h-full">
        <EmptyState title="Select a document">Its source, matching evidence and what is missing are shown here.</EmptyState>
      </Panel>
    );
  }
  const a = c.assessment;
  const cls = MATCH_CLASS[a.class];
  const trust = TRUST[c.source.trust];
  const dl = canDownload(c, view.online);
  const busy = controls.busy || view.progress.running;
  return (
    <Panel title="Document details" bodyClassName="overflow-auto p-3">
      <h3 className="selectable text-sm text-fg">{c.title}</h3>
      <div className="mt-1 flex flex-wrap gap-1.5">
        <Badge className={cls.className}>{cls.label}</Badge>
        <Badge className={trust.className}>{trust.label}</Badge>
        <Badge className="border-line-strong text-muted">{DOC_TYPE[c.doc_type]}</Badge>
      </div>
      <p className="mt-2 text-[11px] text-muted">{a.explanation}</p>
      {a.manual_verification_recommended ? <p className="mt-1 text-[11px] text-orange">Manual verification recommended before relying on this document.</p> : null}

      <section aria-label="Hardware match" className="mt-3">
        <span className="label">Hardware match</span>
        <Row label="Revision">{REVISION[a.revision]}</Row>
        <Row label="Evidence">{`+${a.support_points} supporting / −${a.contradiction_points} contradicting points`}</Row>
        <EvidenceList title="What matched" items={a.evidence.filter((e) => e.effect === "supports")} tone="text-green" />
        <EvidenceList title="What contradicts it" items={a.evidence.filter((e) => e.effect === "contradicts")} tone="text-red" />
        <EvidenceList title="Context" items={a.evidence.filter((e) => e.effect === "neutral")} tone="text-muted" />
        {a.missing.length ? (
          <div className="mt-2">
            <span className="label">Missing information</span>
            <ul>{a.missing.map((m) => <li key={m} className="text-[11px] text-dim">{m}</li>)}</ul>
          </div>
        ) : null}
      </section>

      <section aria-label="Source" className="mt-3">
        <span className="label">Source (separate from the hardware match)</span>
        <Row label="Source">{c.source.source_name}</Row>
        <Row label="Trust">{c.source.trust_reason}</Row>
        <Row label="Found by">{c.source.provider_name}</Row>
        {c.source.page_url ? <Row label="Source page">{c.source.page_url}</Row> : null}
        {c.source.document_url ? <Row label="Document address">{c.source.document_url}</Row> : null}
        {c.source.usage_note ? <Row label="Usage">{c.source.usage_note}</Row> : null}
      </section>

      <section aria-label="File" className="mt-3">
        <span className="label">File</span>
        <Row label="Filename">{c.filename ?? "Not known"}</Row>
        <Row label="Format">{`${c.format}${c.format_basis === "content" ? " (checked from content)" : c.format_basis === "unknown" ? "" : ` (from ${c.format_basis.replace("_", " ")}, not verified)`}`}</Row>
        {c.size_bytes != null ? <Row label="Size">{formatBytes(c.size_bytes)}</Row> : null}
        <Row label="Retrieval">{retrievalText(c.retrieval)}</Row>
        {c.retrieval.state === "downloaded" ? <Row label="SHA-256">{c.retrieval.sha256}</Row> : null}
        {c.metadata.archive_note ? <Row label="Archive">{`${c.metadata.archive_entries ?? "?"} entries. ${c.metadata.archive_note}`}</Row> : null}
        <Row label="Discovered">{formatTime(c.discovered_at)}</Row>
        <Row label="Last verified">{formatTime(c.verified_at)}</Row>
      </section>

      <div className="mt-3 flex flex-wrap gap-2">
        {c.source.page_url || c.source.document_url ? <Button onClick={() => controls.open(c.id, "source_page")}>Open source page</Button> : null}
        {canOpenLocal(c) ? <Button onClick={() => controls.open(c.id, "local_copy")}>Open local copy</Button> : null}
        {c.retrieval.state === "downloaded" ? (
          <Button onClick={() => controls.removeFile(c.id)} disabled={busy}>
            Remove downloaded copy
          </Button>
        ) : (
          <Button onClick={() => controls.download(c.id)} disabled={!dl.ok || busy} title={dl.why ?? undefined} primary>
            Download
          </Button>
        )}
      </div>
      {!dl.ok && c.retrieval.state !== "downloaded" && c.retrieval.state !== "in_library" ? <p className="mt-1 text-[10px] text-dim">Download unavailable: {dl.why}</p> : null}
      <p className="mt-2 text-[10px] text-dim">Sentinel never opens, extracts or runs downloaded files by itself. Documents are information only.</p>
    </Panel>
  );
}

export function SchematicLibraryPage({
  controls,
  serviceRunning,
  onOpenSettings,
  initialSelected = null,
}: {
  controls: DocumentsControls;
  serviceRunning: boolean;
  onOpenSettings: () => void;
  initialSelected?: number | null;
}) {
  const view = controls.view;
  const [selected, setSelected] = useState<number | null>(initialSelected);
  const [showMismatches, setShowMismatches] = useState(false);
  const empty = emptyState(view, serviceRunning);
  const running = view?.progress.running === true;
  const canSearch = serviceRunning && !!view?.identity && !running && !controls.busy;
  const sel = view?.candidates.find((c) => c.id === selected) ?? null;
  const failures = view ? providerFailures(view) : [];
  return (
    <div className="grid min-w-[980px] grid-cols-12 gap-3">
      <div className="col-span-3 flex flex-col gap-3">
        <Overview view={view} />
        <Panel title="Discovery">
          <div className="flex flex-wrap gap-2">
            <Button primary onClick={() => controls.search(false)} disabled={!canSearch} title="Uses recent results when they are still current">
              Search for documentation
            </Button>
            <Button onClick={() => controls.search(true)} disabled={!canSearch}>
              Refresh
            </Button>
            {running ? <Button onClick={controls.cancel}>Cancel</Button> : null}
            <Button onClick={controls.clearCache} disabled={!canSearch || !view?.candidates.length}>
              Clear results
            </Button>
          </div>
          <p className="mt-2 text-[10px] text-dim">
            {view?.online
              ? "Online discovery is on: searches send only the manufacturer, model and board identifier to the configured sources."
              : "Offline: nothing is sent anywhere. Only your document library is searched."}{" "}
            <button type="button" onClick={onOpenSettings} className="text-cyan hover:underline">
              Discovery settings
            </button>
          </p>
          {view?.notice ? (
            <p role="status" className="mt-2 border-l-2 border-blue/60 pl-2 text-[11px] text-muted">
              {view.notice}
            </p>
          ) : null}
        </Panel>
        {view ? <Progress view={view} /> : null}
        {view ? <Providers view={view} /> : null}
      </div>

      {/* Keyed so the results replace the empty state as a new subtree:
          WebKitGTK does not update its accessibility tree when a status
          region is swapped for a list in place. */}
      <Panel key={empty ? `empty-${empty.title}` : "results"} title={`Documents${view ? ` · ${view.candidates.length}` : ""}`} className="col-span-5" bodyClassName="p-0" aside={view?.cache_expires_at ? <span className="label">Results reused until {formatTime(view.cache_expires_at)}</span> : undefined}>
        {failures.length ? <p className="border-b border-line px-3 py-2 text-[11px] text-orange">Provider unavailable: {failures.join("; ")}. Results from other sources are shown.</p> : null}
        {empty ? (
          <div className="p-3">
            <EmptyState title={empty.title}>{empty.body}</EmptyState>
          </div>
        ) : view ? (
          <>
            <p className="border-b border-line px-3 py-2 text-[11px] text-muted">{resultSummary(view)}</p>
            {groups(view.candidates)
              .filter((g) => g.cls !== "mismatch" || showMismatches)
              .map((g) => (
                <section key={g.cls} aria-label={MATCH_CLASS[g.cls].label}>
                  <h3 className="label border-b border-line bg-raised px-3 py-1 text-muted">
                    {MATCH_CLASS[g.cls].label} · {g.items.length}
                  </h3>
                  <ul>
                    {g.items.map((c) => (
                      <ResultItem key={c.id} c={c} selected={c.id === selected} onSelect={() => setSelected(c.id)} />
                    ))}
                  </ul>
                </section>
              ))}
            {view.candidates.some((c) => c.assessment.class === "mismatch") ? (
              <button type="button" onClick={() => setShowMismatches(!showMismatches)} className="m-3 text-[11px] text-muted underline">
                {showMismatches ? "Hide" : "Show"} {view.candidates.filter((c) => c.assessment.class === "mismatch").length} document(s) for other hardware
              </button>
            ) : null}
          </>
        ) : null}
      </Panel>

      <div className="col-span-4 min-w-0">{view ? <Detail c={sel} view={view} controls={controls} /> : null}</div>
    </div>
  );
}
