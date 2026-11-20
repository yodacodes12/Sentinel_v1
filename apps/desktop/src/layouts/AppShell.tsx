import type { ReactNode } from "react";

import { Icon } from "../components/Icon";
import type { BridgeError } from "../types/sentinel";

/** Window frame: header on top, sidebar on the left, scrolling content. */
export function AppShell({
  header,
  sidebar,
  notice,
  error,
  onDismissError,
  children,
}: {
  header: ReactNode;
  sidebar: ReactNode;
  notice: string | null;
  error: BridgeError | null;
  onDismissError: () => void;
  children: ReactNode;
}) {
  return (
    <div className="flex h-full flex-col bg-canvas text-fg">
      {header}
      <div className="flex min-h-0 flex-1">
        {sidebar}
        <main className="flex min-w-0 flex-1 flex-col">
          {notice ? (
            <div role="status" className="flex items-center gap-2 border-b border-line bg-raised px-4 py-2 text-xs text-muted">
              <Icon name="info" size={14} className="shrink-0 text-blue" />
              {notice}
            </div>
          ) : null}
          {error ? (
            <div role="alert" className="flex items-center gap-2 border-b border-red/40 bg-red/10 px-4 py-2 text-xs text-fg">
              <Icon name="warning" size={14} className="shrink-0 text-red" />
              <span className="selectable flex-1">{error.message}</span>
              <button type="button" onClick={onDismissError} className="font-mono text-[11px] uppercase tracking-wider text-muted hover:text-fg">
                Dismiss
              </button>
            </div>
          ) : null}
          <div className="min-h-0 flex-1 overflow-auto p-3">{children}</div>
        </main>
      </div>
    </div>
  );
}
