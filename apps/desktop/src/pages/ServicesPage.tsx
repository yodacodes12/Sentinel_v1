import { useMemo, useState } from "react";

import { CATEGORY_LABEL } from "../app/inventoryView";
import { DiscoveryPanel, OriginBadge } from "../components/DiscoveryPanel";
import { EmptyState, Panel } from "../components/primitives";
import type { JobProgress, UtilitiesView } from "../types/inventory";

/** Read-only list of systemd services. Sentinel never changes them. */
export function ServicesPage({
  utilities,
  progress,
  serviceRunning,
  onScan,
  onCancel,
}: {
  utilities: UtilitiesView | null;
  progress: JobProgress | null;
  serviceRunning: boolean;
  onScan: (force: boolean) => void;
  onCancel: () => void;
}) {
  const [relevantOnly, setRelevantOnly] = useState(true);
  const [search, setSearch] = useState("");
  const catalog = utilities?.catalog ?? null;
  const services = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return (catalog?.services ?? []).filter(
      (s) => (!relevantOnly || s.relevant_category !== null) && (!needle || s.name.toLowerCase().includes(needle) || (s.description ?? "").toLowerCase().includes(needle)),
    );
  }, [catalog, relevantOnly, search]);
  const servicesModule = catalog?.discovery_status.find((r) => r.module === "services");

  return (
    <div className="grid min-w-[980px] grid-cols-12 gap-3">
      <Panel
        title={catalog ? `Services · ${services.length} shown of ${catalog.services.length}` : "Services"}
        className="col-span-9"
        bodyClassName="max-h-[760px] overflow-auto p-0"
        aside={utilities && catalog ? <OriginBadge origin={utilities.origin} source={utilities.source} scannedAt={catalog.discovered_at} /> : undefined}
      >
        {!catalog ? (
          <div className="h-[400px]">
            <EmptyState title="Not scanned yet">Scan for utilities and services to see this list.</EmptyState>
          </div>
        ) : (
          <table className="w-full text-left text-[11px]">
            <thead className="sticky top-0 bg-panel">
              <tr className="border-b border-line">
                {["Service", "Scope", "State at scan", "Enabled", "Category", "Description"].map((h) => (
                  <th key={h} className="label px-2 py-1.5 font-normal">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {services.map((s) => (
                <tr key={`${s.scope}:${s.name}`} className="border-b border-line" title={s.unit_file_path ?? undefined}>
                  <td className="selectable px-2 py-1 font-mono text-fg">{s.name}</td>
                  <td className="px-2 py-1 text-muted">{s.scope}</td>
                  <td className={`px-2 py-1 font-mono ${s.active_state === "active" ? "text-green" : s.active_state === "failed" ? "text-orange" : "text-dim"}`}>
                    {[s.active_state, s.sub_state].filter(Boolean).join(" / ") || "—"}
                  </td>
                  <td className="px-2 py-1 font-mono text-muted">{s.enabled_state ?? "—"}</td>
                  <td className="px-2 py-1 text-muted">{s.relevant_category ? CATEGORY_LABEL[s.relevant_category] : ""}</td>
                  <td className="px-2 py-1 text-muted">{s.description ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>
      <div className="col-span-3 flex flex-col gap-3">
        <Panel title="Filter">
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search services"
            aria-label="Search services"
            className="w-full border border-line-strong bg-canvas px-2 py-1.5 font-mono text-xs text-fg placeholder:text-dim focus:border-cyan"
          />
          <label className="mt-2 flex items-center gap-2 text-xs text-muted">
            <input type="checkbox" checked={relevantOnly} onChange={(e) => setRelevantOnly(e.target.checked)} />
            Hardware, power, storage and network only
          </label>
          {servicesModule && servicesModule.notes.length ? <p className="mt-2 text-[11px] text-orange">{servicesModule.notes.join(" ")}</p> : null}
        </Panel>
        <Panel title="Scan">
          <DiscoveryPanel job="utilities" progress={progress} serviceRunning={serviceRunning} hasResult={catalog !== null} onScan={onScan} onCancel={onCancel} />
        </Panel>
        <Panel title="Read-only">
          <p className="text-[11px] leading-relaxed text-dim">
            This list comes from systemd's own read-only listings. Sentinel never starts, stops, enables or disables a service. States are as
            they were at scan time.
          </p>
        </Panel>
      </div>
    </div>
  );
}
