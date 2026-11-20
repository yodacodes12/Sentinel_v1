import { NAV_ITEMS } from "../app/catalog";
import { Icon } from "./Icon";

export function Sidebar({ activeId, onNavigate }: { activeId: string; onNavigate: (id: string) => void }) {
  return (
    <nav aria-label="Sections" className="flex w-[220px] shrink-0 flex-col border-r border-line bg-panel">
      <ul className="flex-1 py-2">
        {NAV_ITEMS.map((item) => {
          const active = item.id === activeId;
          return (
            <li key={item.id}>
              <button
                type="button"
                aria-current={active ? "page" : undefined}
                onClick={() => onNavigate(item.id)}
                className={`flex h-9 w-full items-center gap-3 border-l-2 px-4 text-left text-[13px] ${
                  active
                    ? "border-cyan bg-cyan/[0.07] text-fg"
                    : "border-transparent text-muted hover:bg-raised hover:text-fg"
                }`}
              >
                <Icon name={item.icon} className={active ? "text-cyan" : "text-dim"} />
                <span className="flex-1 truncate">{item.label}</span>
                {item.implemented ? null : (
                  <span className="font-mono text-[9px] uppercase tracking-widest text-dim" title="In development">
                    dev
                  </span>
                )}
              </button>
            </li>
          );
        })}
      </ul>
      <p className="border-t border-line px-4 py-3 text-[11px] leading-snug text-dim">
        Sentinel observes, analyses and recommends. It never changes your system.
      </p>
    </nav>
  );
}
