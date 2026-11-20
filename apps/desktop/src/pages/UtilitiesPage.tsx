import { useMemo, useState } from "react";

import { CATEGORY_LABEL, SOURCE_LABEL } from "../app/inventoryView";
import { DiscoveryPanel, OriginBadge } from "../components/DiscoveryPanel";
import { EmptyState, Panel } from "../components/primitives";
import type { JobProgress, SystemUtility, UtilitiesView, UtilityCategory } from "../types/inventory";

export function filterUtilities(
  utilities: SystemUtility[],
  { search, category, installedOnly }: { search: string; category: UtilityCategory | "all"; installedOnly: boolean },
): SystemUtility[] {
  const needle = search.trim().toLowerCase();
  return utilities.filter(
    (u) =>
      (!installedOnly || u.installed) &&
      (category === "all" || u.category === category) &&
      (!needle || u.name.toLowerCase().includes(needle) || u.id.toLowerCase().includes(needle) || (u.description ?? "").toLowerCase().includes(needle)),
  );
}

export function UtilitiesPage({
  utilities,
  progress,
  serviceRunning,
  launching,
  onScan,
  onCancel,
  onLaunch,
}: {
  utilities: UtilitiesView | null;
  progress: JobProgress | null;
  serviceRunning: boolean;
  launching: string | null;
  onScan: (force: boolean) => void;
  onCancel: () => void;
  onLaunch: (id: string) => void;
}) {
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState<UtilityCategory | "all">("all");
  const [installedOnly, setInstalledOnly] = useState(true);
  const catalog = utilities?.catalog ?? null;
  const shown = useMemo(() => filterUtilities(catalog?.utilities ?? [], { search, category, installedOnly }), [catalog, search, category, installedOnly]);
  const installed = catalog?.utilities.filter((u) => u.installed).length ?? 0;

  return (
    <div className="grid min-w-[980px] grid-cols-12 gap-3">
      <div className="col-span-3 flex flex-col gap-3">
        <Panel title="Filter">
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search by name"
            aria-label="Search utilities"
            className="w-full border border-line-strong bg-canvas px-2 py-1.5 font-mono text-xs text-fg placeholder:text-dim focus:border-cyan"
          />
          <label className="mt-2 flex items-center gap-2 text-xs text-muted">
            <input type="checkbox" checked={installedOnly} onChange={(e) => setInstalledOnly(e.target.checked)} />
            Installed only
          </label>
          <ul className="mt-2">
            {(["all", ...Object.keys(CATEGORY_LABEL)] as (UtilityCategory | "all")[]).map((c) => (
              <li key={c}>
                <button
                  type="button"
                  onClick={() => setCategory(c)}
                  aria-pressed={category === c}
                  className={`w-full border-b border-line px-1 py-1 text-left text-[11px] ${category === c ? "text-cyan" : "text-muted hover:text-fg"}`}
                >
                  {c === "all" ? "All categories" : CATEGORY_LABEL[c]}
                  <span className="float-right font-mono text-dim">
                    {catalog ? catalog.utilities.filter((u) => (c === "all" || u.category === c) && (!installedOnly || u.installed)).length : ""}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </Panel>
        <Panel title="Utility scan">
          <DiscoveryPanel job="utilities" progress={progress} serviceRunning={serviceRunning} hasResult={catalog !== null} onScan={onScan} onCancel={onCancel} />
        </Panel>
      </div>

      <Panel
        title={catalog ? `Installed utilities · ${installed} of ${catalog.utilities.length} known` : "Installed utilities"}
        className="col-span-9"
        bodyClassName="max-h-[760px] overflow-auto p-0"
        aside={utilities && catalog ? <OriginBadge origin={utilities.origin} source={utilities.source} scannedAt={catalog.discovered_at} /> : undefined}
      >
        {!catalog ? (
          <div className="h-[400px]">
            <EmptyState title="Not scanned yet">
              {serviceRunning ? "Scan for utilities to see which system tools are installed." : "Start the Sentinel service, then scan for utilities."}
            </EmptyState>
          </div>
        ) : shown.length === 0 ? (
          <div className="h-[200px]">
            <EmptyState title="No utilities match" />
          </div>
        ) : (
          <ul>
            {shown.map((u) => (
              <li key={u.id} className="grid grid-cols-[1fr_auto] items-center gap-3 border-b border-line px-3 py-2">
                <div className="min-w-0">
                  <div className="flex items-baseline gap-2">
                    <span className={`text-sm ${u.installed ? "text-fg" : "text-dim"}`}>{u.name}</span>
                    <span className="font-mono text-[10px] text-dim">{u.id}</span>
                    {u.version ? <span className="font-mono text-[10px] text-muted">{u.version}</span> : null}
                  </div>
                  <div className="truncate text-[11px] text-muted">{u.description ?? ""}</div>
                  <div className="mt-0.5 flex flex-wrap gap-x-3 text-[10px] text-dim">
                    <span>{CATEGORY_LABEL[u.category]}</span>
                    <span>{u.installed ? `Found via: ${u.sources.map((s) => SOURCE_LABEL[s]).join(", ")}` : "Not installed"}</span>
                    {u.package ? <span>{`${u.package.manager} package ${u.package.name}`}</span> : null}
                    {u.executable ? <span className="selectable font-mono">{u.executable}</span> : null}
                  </div>
                </div>
                <div className="flex flex-col items-end gap-1">
                  {u.installed && u.launch_available ? (
                    <button
                      type="button"
                      disabled={launching !== null}
                      onClick={() => onLaunch(u.id)}
                      className="border border-cyan/60 px-2 py-1 font-mono text-[11px] uppercase tracking-wider text-cyan hover:bg-cyan/10 disabled:opacity-50"
                    >
                      {launching === u.id ? "Opening…" : "Open"}
                    </button>
                  ) : (
                    <span className="max-w-[220px] text-right text-[10px] text-dim">{u.launch_note ?? ""}</span>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
        {catalog ? (
          <p className="p-3 text-[10px] text-dim">
            Sentinel only opens applications you choose, through your desktop's own launcher. It never installs, removes or runs command-line
            tools.
          </p>
        ) : null}
      </Panel>
    </div>
  );
}
