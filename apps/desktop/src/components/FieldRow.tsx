import type { ReactNode } from "react";

import { RELIABILITY } from "../app/explorer";
import { CONFIDENCE_TEXT, show } from "../app/inventoryView";
import type { Field } from "../types/inventory";

/**
 * One discovered value with its provenance. Missing values say why they are
 * missing; inferred values are marked; the source path is always visible.
 */
export function FieldRow<T>({
  label,
  field,
  render,
  sensitive,
}: {
  label: string;
  field: Field<T> | null | undefined;
  render?: (value: T) => string;
  /** Rendered in place of the value (e.g. a masked MAC address). */
  sensitive?: ReactNode;
}) {
  const shown = show(field, render);
  const tone = field ? RELIABILITY[field.confidence].tone : "solid";
  return (
    <div className="grid grid-cols-[150px_1fr_auto] items-baseline gap-3 border-b border-line py-1.5 last:border-b-0">
      <span className="text-xs text-muted">{label}</span>
      <span className="min-w-0">
        <span className={`selectable block truncate font-mono text-xs ${shown.present ? "text-fg" : "text-dim"}`} title={field?.notes}>
          {shown.present && sensitive ? sensitive : shown.text}
        </span>
        {field?.notes ? <span className="block truncate text-[10px] text-dim">{field.notes}</span> : null}
      </span>
      <span className="flex items-center gap-1.5" title={field?.source ? `Source: ${field.source}` : undefined}>
        {field && shown.present ? (
          <span
            className={`border px-1 py-px font-mono text-[9px] uppercase tracking-wider ${
              tone === "inferred" ? "border-orange/60 text-orange" : tone === "derived" ? "border-blue/60 text-blue" : "border-line-strong text-muted"
            }`}
            title={field ? RELIABILITY[field.confidence].meaning : undefined}
          >
            {CONFIDENCE_TEXT[field.confidence]}
          </span>
        ) : null}
        {field?.source ? <span className="selectable max-w-[220px] truncate font-mono text-[10px] text-dim">{field.source}</span> : null}
      </span>
    </div>
  );
}

/** A plain label/value row for values without a Field wrapper. */
export function PlainRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[150px_1fr] items-baseline gap-3 border-b border-line py-1.5 last:border-b-0">
      <span className="text-xs text-muted">{label}</span>
      <span className="selectable min-w-0 truncate font-mono text-xs text-fg" title={typeof children === "string" ? children : undefined}>
        {children}
      </span>
    </div>
  );
}
