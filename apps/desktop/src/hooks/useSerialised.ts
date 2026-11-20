import { useMemo, useRef } from "react";

import { serialise } from "../services/serialise";

/** [`serialise`] for a hook: always runs the latest `task`. */
export function useSerialised(task: () => Promise<void>): () => Promise<void> {
  const latest = useRef(task);
  latest.current = task;
  return useMemo(() => serialise(() => latest.current()), []);
}
