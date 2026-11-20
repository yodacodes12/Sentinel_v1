import { useMemo, useRef, useState, type ReactNode } from "react";

import { PARENT_BASIS_TEXT, ancestors, treeKey, visibleTree, type IndexMaps } from "../app/explorer";
import type { ComponentNode } from "../types/inventory";

/**
 * An expandable tree of components following the WAI-ARIA tree pattern:
 * one tab stop, arrow keys to move, right/left to expand/collapse, Enter or
 * Space to select. Relationships are drawn as reported; a component whose
 * parent was not discovered is listed at the top level and marked.
 */
export function ComponentTree({
  maps,
  roots,
  selectedId,
  onSelect,
  label,
  detail,
  defaultDepth = 1,
}: {
  maps: IndexMaps;
  roots: ComponentNode[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  label: string;
  /** Extra text after a node's name (IDs, sizes, live values). */
  detail?: (node: ComponentNode) => ReactNode;
  /** Levels expanded initially. */
  defaultDepth?: number;
}) {
  const initial = useMemo(() => {
    const open = new Set<string>();
    const walk = (n: ComponentNode, depth: number) => {
      if (depth >= defaultDepth) return;
      open.add(n.id);
      n.children.forEach((c) => {
        const child = maps.byId.get(c);
        if (child) walk(child, depth + 1);
      });
    };
    roots.forEach((r) => walk(r, 0));
    return open;
  }, [maps, roots, defaultDepth]);
  const [expanded, setExpanded] = useState<Set<string>>(initial);
  const [focused, setFocused] = useState<string | null>(null);
  const refs = useRef(new Map<string, HTMLLIElement>());

  // A selection made elsewhere (a diagram, a link) is revealed in the tree.
  const open = useMemo(() => {
    if (!selectedId || !maps.byId.has(selectedId)) return expanded;
    const path = ancestors(maps, selectedId);
    return path.every((id) => expanded.has(id)) ? expanded : new Set([...expanded, ...path]);
  }, [expanded, selectedId, maps]);

  const visible = visibleTree(maps, roots, open);
  const tabStop = focused && visible.includes(focused) ? focused : selectedId && visible.includes(selectedId) ? selectedId : (visible[0] ?? null);

  const toggle = (id: string, to?: boolean) =>
    setExpanded(() => {
      const next = new Set(open);
      if (to ?? !next.has(id)) next.add(id);
      else next.delete(id);
      return next;
    });

  const onKey = (event: React.KeyboardEvent, id: string) => {
    const action = treeKey(maps, visible, open, id, event.key);
    if (!action) return;
    event.preventDefault();
    event.stopPropagation();
    if ("focus" in action) {
      setFocused(action.focus);
      refs.current.get(action.focus)?.focus();
    } else if ("expand" in action) toggle(action.expand, true);
    else if ("collapse" in action) toggle(action.collapse, false);
    else onSelect(action.select);
  };

  const render = (node: ComponentNode, level: number): ReactNode => {
    const hasChildren = node.children.length > 0;
    const isOpen = open.has(node.id);
    const selected = node.id === selectedId;
    return (
      <li
        key={node.id}
        ref={(el) => {
          if (el) refs.current.set(node.id, el);
          else refs.current.delete(node.id);
        }}
        role="treeitem"
        aria-level={level}
        aria-selected={selected}
        aria-expanded={hasChildren ? isOpen : undefined}
        aria-label={node.name}
        tabIndex={node.id === tabStop ? 0 : -1}
        onKeyDown={(e) => onKey(e, node.id)}
        onFocus={(e) => {
          if (e.target === e.currentTarget) setFocused(node.id);
        }}
        className="outline-none focus-visible:[&>div]:ring-1 focus-visible:[&>div]:ring-cyan"
      >
        <div
          className={`flex cursor-pointer items-center gap-1.5 border-l-2 py-[3px] pr-2 text-xs ${selected ? "border-cyan bg-cyan/10 text-fg" : "border-transparent text-muted hover:bg-raised hover:text-fg"}`}
          style={{ paddingLeft: 6 + (level - 1) * 14 }}
          onClick={() => onSelect(node.id)}
        >
          {hasChildren ? (
            <button
              type="button"
              tabIndex={-1}
              aria-hidden="true"
              className="w-3 shrink-0 font-mono text-[10px] text-dim hover:text-fg"
              onClick={(e) => {
                e.stopPropagation();
                toggle(node.id);
              }}
            >
              {isOpen ? "▾" : "▸"}
            </button>
          ) : (
            <span className="w-3 shrink-0" />
          )}
          <span className="truncate">{node.name}</span>
          {node.parent_basis === "parent_not_found" ? (
            <span className="shrink-0 border border-dashed border-orange/60 px-1 font-mono text-[9px] uppercase text-orange" title="The reported parent was not discovered, so this component is not placed under anything.">
              {PARENT_BASIS_TEXT.parent_not_found}
            </span>
          ) : null}
          {detail ? <span className="ml-auto shrink-0 pl-2 font-mono text-[10px] text-dim">{detail(node)}</span> : null}
        </div>
        {hasChildren && isOpen ? (
          <ul role="group">
            {node.children.map((c) => {
              const child = maps.byId.get(c);
              return child ? render(child, level + 1) : null;
            })}
          </ul>
        ) : null}
      </li>
    );
  };

  if (roots.length === 0) return null;
  return (
    <ul role="tree" aria-label={label} className="py-1">
      {roots.map((r) => render(r, 1))}
    </ul>
  );
}
