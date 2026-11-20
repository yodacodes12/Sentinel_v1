import type { ReactNode } from "react";

import type { Accent } from "../app/catalog";
import type { Tone } from "../app/viewModel";

const TONE_TEXT: Record<Tone, string> = {
  ok: "text-green",
  idle: "text-muted",
  warn: "text-orange",
  error: "text-red",
  unknown: "text-dim",
};

const TONE_DOT: Record<Tone, string> = {
  ok: "bg-green",
  idle: "bg-dim",
  warn: "bg-orange",
  error: "bg-red",
  unknown: "bg-line-strong",
};

export const ACCENT_TEXT: Record<Accent, string> = {
  cyan: "text-cyan",
  blue: "text-blue",
  orange: "text-orange",
  purple: "text-purple",
  green: "text-green",
};

/** A bordered charcoal panel with a small technical header. */
export function Panel({
  title,
  aside,
  children,
  className = "",
  bodyClassName = "p-3",
}: {
  title: string;
  aside?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section className={`flex min-h-0 flex-col border border-line bg-panel ${className}`}>
      <header className="flex h-8 shrink-0 items-center justify-between border-b border-line px-3">
        <h2 className="label text-muted">{title}</h2>
        {aside}
      </header>
      <div className={`min-h-0 flex-1 ${bodyClassName}`}>{children}</div>
    </section>
  );
}

/** Coloured dot plus label, used wherever a state is reported. */
export function StatusDot({ tone, label }: { tone: Tone; label: string }) {
  return (
    <span className={`inline-flex items-center gap-2 font-mono text-xs ${TONE_TEXT[tone]}`}>
      <span className={`h-1.5 w-1.5 ${TONE_DOT[tone]}`} />
      {label}
    </span>
  );
}

/**
 * Marks content that is not live data. Hatched and orange so it can never be
 * mistaken for a reading taken from this computer.
 */
export function Tag({ children, kind }: { children: ReactNode; kind: "illustrative" | "development" | "stored" }) {
  const style =
    kind === "stored"
      ? "border-line-strong text-muted"
      : "border-orange/60 text-orange bg-[repeating-linear-gradient(135deg,transparent_0_4px,rgb(255_159_67/0.10)_4px_8px)]";
  return (
    <span className={`inline-flex items-center border px-1.5 py-px font-mono text-[10px] uppercase tracking-widest ${style}`}>
      {children}
    </span>
  );
}

/** A label/value row for facts. `value` of null renders an honest dash. */
export function Fact({ label, value, mono = true }: { label: string; value: ReactNode | null; mono?: boolean }) {
  return (
    <div className="flex items-baseline justify-between gap-3 border-b border-line py-1.5 last:border-b-0">
      <span className="shrink-0 text-xs text-muted">{label}</span>
      <span className={`selectable min-w-0 truncate text-right text-xs ${mono ? "font-mono" : ""} ${value === null ? "text-dim" : "text-fg"}`}>
        {value ?? "—"}
      </span>
    </div>
  );
}

export function EmptyState({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div role="status" aria-live="off" aria-label={title} className="flex h-full flex-col items-center justify-center gap-1 px-4 text-center">
      <span className="label">{title}</span>
      {children ? <p className="max-w-sm text-xs text-dim">{children}</p> : null}
    </div>
  );
}

/** Segmented control for a small set of exclusive options. */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  disabled,
  label,
}: {
  options: readonly { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <div role="radiogroup" aria-label={label} className="grid auto-cols-fr grid-flow-col border border-line-strong">
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            disabled={disabled}
            onClick={() => !selected && onChange(option.value)}
            className={`border-r border-line-strong px-2 py-1.5 font-mono text-[11px] uppercase tracking-wider last:border-r-0 disabled:opacity-50 ${
              selected ? "bg-cyan/10 text-cyan" : "text-muted hover:bg-raised hover:text-fg"
            }`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

/** A two-state switch. The label states what it controls and its state. */
export function Switch({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-5 w-10 shrink-0 border transition-colors duration-150 disabled:opacity-50 ${
        checked ? "border-cyan bg-cyan/15" : "border-line-strong bg-raised"
      }`}
    >
      <span
        className={`absolute top-0.5 h-3.5 w-3.5 transition-[left] duration-150 ${
          checked ? "left-[22px] bg-cyan" : "left-0.5 bg-dim"
        }`}
      />
    </button>
  );
}
