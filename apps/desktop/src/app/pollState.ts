// How background polling changes what the UI shows. Pure functions, so the
// rules are tested without a running window.

import type { BridgeError } from "../types/sentinel";

/** The daemon is not there at all (stopped, crashed, restarting). */
export const DAEMON_UNAVAILABLE = "daemon_unavailable";

export interface PollStatus {
  error: BridgeError | null;
  /** `error` came from background polling, not from something the user did. */
  pollError: boolean;
  /** The last poll failed: the data shown is the last answer received, not live. */
  stale: boolean;
}

/** A poll answered: the data is live again and a polling error no longer applies. */
export function afterPollSuccess(s: PollStatus): PollStatus {
  return { error: s.pollError ? null : s.error, pollError: false, stale: false };
}

/**
 * A poll failed. With no daemon there is nothing live to show and the
 * header already says so: the data is dropped, with no banner. Any other
 * failure (timeout, garbled answer) keeps the last data but marks it stale
 * and reports the error, unless an error from the user's own action is
 * already shown.
 */
export function afterPollFailure(s: PollStatus, error: BridgeError): PollStatus & { dropData: boolean } {
  if (error.kind === DAEMON_UNAVAILABLE) {
    return { ...afterPollSuccess(s), dropData: true };
  }
  const userError = s.error !== null && !s.pollError;
  return { error: userError ? s.error : error, pollError: !userError, stale: true, dropData: false };
}
