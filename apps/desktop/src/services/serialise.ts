/**
 * Wrap `run` so that calls never overlap: a call made while one is in
 * flight is coalesced into a single follow-up run, and every caller's
 * promise settles when the work it asked for is done. A slow or hung
 * daemon therefore cannot pile up requests, and answers cannot be applied
 * out of order.
 */
export function serialise(run: () => Promise<void>): () => Promise<void> {
  let running: Promise<void> | null = null;
  let again = false;
  return () => {
    if (running) {
      again = true;
      return running;
    }
    running = (async () => {
      try {
        do {
          again = false;
          await run();
        } while (again);
      } finally {
        running = null;
      }
    })();
    return running;
  };
}
