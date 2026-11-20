import type { NavItem } from "../app/catalog";
import { Icon } from "../components/Icon";
import { Tag } from "../components/primitives";

/** Shown for every section that is not built yet. It shows no data at all. */
export function PlaceholderPage({ item }: { item: NavItem }) {
  return (
    <div className="flex h-full items-center justify-center border border-line bg-panel">
      <div className="flex max-w-md flex-col items-center gap-4 px-6 text-center">
        <Icon name={item.icon} size={40} className="text-dim" />
        <h1 className="font-mono text-base tracking-[0.2em] text-fg">{item.label.toUpperCase()}</h1>
        <Tag kind="development">In development</Tag>
        <p className="text-sm text-muted">{item.planned}</p>
        <p className="text-xs text-dim">
          This section is not part of the current release. Nothing is collected or displayed here yet.
        </p>
      </div>
    </div>
  );
}
