import { ACCENT_HEX } from "../app/catalog";
import { CORE_ACCENT, topologyLayout } from "../app/inventoryView";
import type { ProcessorInfo } from "../types/inventory";

/**
 * Topology of one processor package, drawn from the discovered cores and
 * logical CPUs. It is a logical diagram, not a picture of the die.
 */
export function CpuTopology({
  processor,
  selectedCore,
  onSelect,
  usage,
}: {
  processor: ProcessorInfo;
  selectedCore: number | null;
  onSelect: (index: number) => void;
  /** Live utilisation per logical CPU, when monitoring is active. */
  usage?: Map<number, number | null>;
}) {
  if (processor.cores.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-dim">
        The kernel did not expose this processor's core topology, so no diagram is drawn.
      </div>
    );
  }
  const layout = topologyLayout(processor);
  return (
    <svg
      viewBox={`0 0 ${layout.width} ${layout.height + 22}`}
      className="h-full w-full"
      role="group"
      aria-label={`Processor package ${processor.package_id}: ${processor.cores.length} cores`}
    >
      <rect x="2" y="2" width={layout.width - 4} height={layout.height - 4} fill="none" stroke="#2c3541" strokeWidth="1.5" />
      <text x="10" y={layout.height + 14} fill="#5f6b78" fontSize="10" fontFamily="var(--font-mono)" letterSpacing="1.5">
        {`PACKAGE ${processor.package_id} · ${processor.cores.length} CORES · ${processor.cores.reduce((n, c) => n + c.logical_cpus.length, 0)} LOGICAL CPUS · TOPOLOGY, NOT A DIE IMAGE`}
      </text>
      {layout.cores.map((box) => {
        const color = ACCENT_HEX[CORE_ACCENT[box.core.kind]];
        const selected = box.core.index === selectedCore;
        return (
          <g
            key={box.core.index}
            role="button"
            tabIndex={0}
            aria-label={`Core ${box.core.index}, logical CPUs ${box.core.logical_cpus.join(", ")}`}
            aria-pressed={selected}
            onClick={() => onSelect(box.core.index)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onSelect(box.core.index);
              }
            }}
            className="cursor-pointer outline-none"
          >
            <rect
              x={box.x}
              y={box.y}
              width={box.width}
              height={box.height}
              fill={selected ? `${color}22` : "#0f1318"}
              stroke={color}
              strokeOpacity={selected ? 1 : 0.55}
              strokeWidth={selected ? 1.5 : 1}
            />
            <text x={box.x + 6} y={box.y + 16} fill={color} fontSize="10" fontFamily="var(--font-mono)" letterSpacing="1">
              {`CORE ${box.core.index}${box.core.kind === "performance" ? " · P" : box.core.kind === "efficiency" ? " · E" : ""}`}
            </text>
            {box.threads.map((thread) => (
              <g key={thread.cpu}>
                <rect x={thread.x} y={thread.y} width={thread.width} height={thread.height} fill="#151a21" stroke="#2c3541" />
                {usage?.get(thread.cpu) != null ? (
                  <rect x={thread.x} y={thread.y} width={(thread.width * Math.min(100, usage.get(thread.cpu)!)) / 100} height={thread.height} fill={color} fillOpacity="0.28" />
                ) : null}
                <text x={thread.x + 5} y={thread.y + 12.5} fill="#9aa6b2" fontSize="9.5" fontFamily="var(--font-mono)">
                  {`CPU ${thread.cpu}`}
                </text>
                {usage ? (
                  <text x={thread.x + thread.width - 5} y={thread.y + 12.5} fill="#f2f5f8" fontSize="9.5" fontFamily="var(--font-mono)" textAnchor="end">
                    {usage.get(thread.cpu) != null ? `${Math.round(usage.get(thread.cpu)!)}%` : "—"}
                  </text>
                ) : null}
              </g>
            ))}
          </g>
        );
      })}
    </svg>
  );
}
