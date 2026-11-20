import { KIND_TEXT, PARENT_BASIS_TEXT, RELATION_TEXT, liveFor, specRows, updatedText, type ExplorerTelemetry, type IndexMaps } from "../app/explorer";
import type { ComponentNode, SystemInventory } from "../types/inventory";
import { EmptyState, Panel } from "./primitives";
import { ReliabilityBadge } from "./Reliability";

const CATEGORY_TEXT: Record<ComponentNode["category"], string> = {
  processor: "Processor",
  memory: "Memory",
  graphics: "Graphics",
  storage: "Storage",
  battery: "Battery and power",
  network: "Network",
  usb: "USB",
  pci: "PCI",
  motherboard: "Motherboard",
};

/**
 * Details of one selected component, the same layout for every kind:
 * identity, live readings (when monitoring is on), specifications with
 * their reliability, relationships with how they are known, and when the
 * information was obtained.
 */
export function Inspector({
  node,
  lost,
  inventory,
  maps,
  telemetry,
  onSelect,
  className = "",
}: {
  node: ComponentNode | null;
  lost: boolean;
  inventory: SystemInventory;
  maps: IndexMaps;
  telemetry: ExplorerTelemetry | null;
  onSelect: (id: string) => void;
  className?: string;
}) {
  if (!node) {
    return (
      <Panel title="Inspector" className={className}>
        <EmptyState title={lost ? "Component no longer detected" : "Select a component"}>
          {lost
            ? "The selected component is not in the current inventory. It may have been removed or renamed since the last scan."
            : "Choose a component in the tree or a diagram to see its specifications, live readings and where each value came from."}
        </EmptyState>
      </Panel>
    );
  }
  const live = liveFor(node, telemetry);
  const specs = specRows(node, inventory, maps);
  const parent = node.parent ? maps.byId.get(node.parent) : undefined;
  const linkButton = (id: string, text: string) => (
    <button key={id} type="button" onClick={() => onSelect(id)} className="mr-2 text-left font-mono text-[11px] text-cyan hover:underline">
      {text}
    </button>
  );

  return (
    <Panel title="Inspector" className={className} bodyClassName="overflow-auto p-3" aside={<span className="label">{CATEGORY_TEXT[node.category]}</span>}>
      <div className="mb-3">
        <h3 className="selectable text-sm text-fg">{node.name}</h3>
        <p className="font-mono text-[10px] text-dim">
          {KIND_TEXT[node.kind]} · <span className="selectable">{node.id}</span>
        </p>
      </div>

      {node.telemetry.length > 0 ? (
        <section aria-label="Live readings" className="mb-3">
          <div className="mb-1 flex items-center justify-between">
            <span className="label">Live</span>
            <span role="status" aria-label={`Live state ${live.state}`} className={`font-mono text-[10px] uppercase ${live.state === "live" ? "text-blue" : live.state === "stale" ? "text-orange" : "text-dim"}`}>
              {live.state === "live" ? "Live" : live.state === "stale" ? "Stale · last values received" : "Monitoring off"}
            </span>
          </div>
          {live.state === "off" ? (
            <p className="text-[11px] text-dim">Turn monitoring on to see live readings for this component. Nothing is measured while it is off.</p>
          ) : live.rows.length === 0 ? (
            <p className="text-[11px] text-dim">No readings for this component in the current sample.</p>
          ) : (
            <ul>
              {live.rows.map((r, i) => (
                <li key={`${r.label}-${i}`} className="grid grid-cols-[130px_1fr_auto] items-baseline gap-2 border-b border-line py-1 last:border-b-0">
                  <span className="text-[11px] text-muted">{r.label}</span>
                  <span className={`selectable font-mono text-xs ${r.present ? (live.state === "stale" ? "text-muted" : "text-fg") : "text-dim"}`}>{r.text}</span>
                  {r.qualifier ? (
                    <span className={`border px-1 py-px font-mono text-[9px] uppercase tracking-wider ${r.qualifier === "derived" ? "border-blue/60 text-blue" : "border-orange/60 text-orange"}`}>
                      {r.qualifier === "derived" ? "Derived" : "Estimate"}
                    </span>
                  ) : (
                    <span />
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>
      ) : null}

      <section aria-label="Specifications" className="mb-3">
        <span className="label">Specifications</span>
        {specs.length === 0 ? (
          <p className="text-[11px] text-dim">No record for this component in the inventory.</p>
        ) : (
          <ul className="mt-1">
            {specs.map((s, i) => (
              <li key={`${s.label}-${i}`} className="grid grid-cols-[130px_1fr_auto] items-baseline gap-2 border-b border-line py-1 last:border-b-0">
                <span className="text-[11px] text-muted">{s.label}</span>
                <span className="min-w-0">
                  <span className={`selectable block break-words font-mono text-xs ${s.present ? "text-fg" : "text-dim"}`}>{s.text}</span>
                  {s.note ? <span className="block text-[10px] text-dim">{s.note}</span> : null}
                </span>
                <ReliabilityBadge availability={s.availability} confidence={s.confidence} source={s.source} />
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-label="Relationships" className="mb-3">
        <span className="label">Relationships</span>
        <dl className="mt-1 grid grid-cols-[130px_1fr] gap-x-2 gap-y-1 text-[11px]">
          <dt className="text-muted">Parent</dt>
          <dd>
            {parent ? linkButton(parent.id, parent.name) : <span className="text-dim">None</span>}
            <span className={`block text-[10px] ${node.parent_basis === "parent_not_found" ? "text-orange" : "text-dim"}`}>{PARENT_BASIS_TEXT[node.parent_basis]}</span>
          </dd>
          {node.children.length ? (
            <>
              <dt className="text-muted">Contains</dt>
              <dd>{node.children.length > 12 ? <span className="font-mono text-dim">{node.children.length} components</span> : node.children.map((c) => linkButton(c, maps.byId.get(c)?.name ?? c))}</dd>
            </>
          ) : null}
          {node.links.map((l) => (
            <div key={`${l.relation}-${l.id}`} className="contents">
              <dt className="text-muted">{RELATION_TEXT[l.relation]}</dt>
              <dd>{linkButton(l.id, maps.byId.get(l.id)?.name ?? l.id)}</dd>
            </div>
          ))}
        </dl>
        <p className="mt-1 text-[10px] text-dim">Logical relationships as the kernel or firmware reports them. None of them gives a physical position.</p>
      </section>

      <p className="text-[10px] text-dim">{updatedText(inventory, live)}</p>
    </Panel>
  );
}
