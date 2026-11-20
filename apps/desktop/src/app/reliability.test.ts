// Phase 4: how the window behaves when the daemon is slow, gone or garbled.
// Pure logic; the same behaviour is exercised in a real browser and in the
// real desktop window by the Phase 4 end-to-end runs (docs/DESKTOP_INTEGRATION.md).

import { describe, expect, it } from "vitest";

import { serialise } from "../services/serialise";
import { DAEMON_UNAVAILABLE, afterPollFailure, afterPollSuccess, type PollStatus } from "./pollState";

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => (resolve = r));
  return { promise, resolve };
}

describe("serialised polling", () => {
  it("never runs two requests at once and coalesces callers into one follow-up", async () => {
    let inFlight = 0;
    let maxInFlight = 0;
    let runs = 0;
    const gates: ReturnType<typeof deferred>[] = [];
    const poll = serialise(async () => {
      runs += 1;
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      const gate = deferred();
      gates.push(gate);
      await gate.promise;
      inFlight -= 1;
    });

    // A hung daemon: ten timer ticks while the first request is stuck.
    const calls = Array.from({ length: 10 }, () => poll());
    await tick();
    expect(runs).toBe(1);
    gates[0]!.resolve();
    await tick();
    // Exactly one follow-up for all the ticks that arrived meanwhile.
    expect(runs).toBe(2);
    gates[1]!.resolve();
    await Promise.all(calls);
    expect(runs).toBe(2);
    expect(maxInFlight).toBe(1);

    // Idle again: the next call runs at once.
    const next = poll();
    await tick();
    expect(runs).toBe(3);
    gates[2]!.resolve();
    await next;
  });

  it("recovers after a failed run", async () => {
    let runs = 0;
    const poll = serialise(async () => {
      runs += 1;
      if (runs === 1) throw new Error("boom");
    });
    await expect(poll()).rejects.toThrow("boom");
    await poll();
    expect(runs).toBe(2);
  });
});

describe("poll results", () => {
  const clean: PollStatus = { error: null, pollError: false, stale: false };
  const timeout = { kind: "timeout", message: "timed out while waiting for the daemon" };
  const refused = { kind: "invalid_config", message: "cpu_high_percent must be between 0 and 100" };

  it("a daemon that went away drops live data without a banner", () => {
    const r = afterPollFailure(clean, { kind: DAEMON_UNAVAILABLE, message: "not running" });
    expect(r.dropData).toBe(true);
    expect(r.error).toBeNull();
    expect(r.stale).toBe(false);
  });

  it("a slow or garbled answer keeps the last data, marked stale, with the reason", () => {
    for (const error of [timeout, { kind: "protocol", message: "the daemon's answer could not be read" }]) {
      const r = afterPollFailure(clean, error);
      expect(r.dropData).toBe(false);
      expect(r.stale).toBe(true);
      expect(r.error).toEqual(error);
      expect(r.pollError).toBe(true);
    }
  });

  it("the next good poll clears a polling error and the stale mark", () => {
    const failed = afterPollFailure(clean, timeout);
    expect(afterPollSuccess(failed)).toEqual(clean);
    // A daemon that has stopped clears it too: the header takes over.
    expect(afterPollFailure(failed, { kind: DAEMON_UNAVAILABLE, message: "" }).error).toBeNull();
  });

  it("an error from the user's own action stays until dismissed", () => {
    const userFailed: PollStatus = { error: refused, pollError: false, stale: false };
    const afterTimeout = afterPollFailure(userFailed, timeout);
    expect(afterTimeout.error).toEqual(refused);
    expect(afterTimeout.stale).toBe(true);
    expect(afterPollSuccess(afterTimeout).error).toEqual(refused);
  });
});
