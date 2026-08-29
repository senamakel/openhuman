/**
 * Whether this user has set up a tiny.place identity (#5424).
 *
 * tiny.place is being removed from the app after 31 August 2026. Its entry
 * points must be shown only to users who already have an identity — everyone
 * else never really started, so there is nothing to preserve and no reason to
 * advertise a feature that is about to disappear.
 *
 * The authoritative signal is the orchestration self-identity RPC: a non-empty
 * `agentId` means the wallet-backed identity exists.
 *
 * Failure handling is deliberate. A *resolved* call (identity present or absent)
 * is terminal and cached for the session. A *rejected* call is treated as
 * transient — the wallet/keyring can be locked during startup, or the relay can
 * blip — so it must NOT permanently hide tiny.place from a real holder. On
 * rejection we stay fail-closed (hidden) for the moment but keep retrying with a
 * bounded backoff, and re-attempt when the window regains focus (e.g. after the
 * user unlocks their wallet), so a one-time startup failure never locks a holder
 * out until an app restart.
 *
 * The check is shared module-side, so the several gates that read it (the
 * Brain orchestration sub-tab, the notice) share a single in-flight request
 * rather than each firing their own.
 */
import { useEffect, useState } from 'react';

import { orchestrationClient } from '../lib/orchestration/orchestrationClient';
import { resolveWalletConfigured } from './useWalletConfigured';

export interface TinyPlaceIdentityState {
  /** `loading` until the RPC first settles; `ready` once we have an answer. */
  status: 'loading' | 'ready';
  /** True only when a tiny.place identity exists for this user. */
  hasIdentity: boolean;
}

/** Bounded backoff (ms) between retries after a transient failure. */
const RETRY_DELAYS_MS = [2000, 5000, 10000];

let cache: TinyPlaceIdentityState = { status: 'loading', hasIdentity: false };
let resolved = false; // the RPC returned a definitive answer — stop retrying
let inFlight = false;
let retryTimer: ReturnType<typeof setTimeout> | null = null;
const listeners = new Set<() => void>();

function publish(next: TinyPlaceIdentityState) {
  cache = next;
  listeners.forEach(listener => listener());
}

async function attemptLoad(attempt: number) {
  if (resolved || inFlight) return;
  inFlight = true;
  try {
    // Gate on wallet status before the wallet-requiring call. `selfIdentity()`
    // builds the tiny.place client from a wallet-derived seed, so for a user
    // with no wallet it can only reject — and the retry ladder below then
    // repeats that rejection, which is how one wallet-less session produced 55
    // error-level reports in 72 minutes (#5805). `wallet_status` answers the
    // same question and resolves normally, so ask it instead of provoking the
    // error. Only a positive `no` skips: `unknown` (status fetch itself failed)
    // falls through so a transport blip never hides a real identity.
    //
    // Deliberately does NOT set `resolved` — that flag ends retrying forever,
    // and a user who sets up a wallet later in the session must still be picked
    // up by the next `ensureLoad()`. Returning without scheduling a retry is
    // enough to stop the ladder.
    if ((await resolveWalletConfigured()) === 'no') {
      publish({ status: 'ready', hasIdentity: false });
      return;
    }
    const identity = await orchestrationClient.selfIdentity();
    resolved = true;
    if (retryTimer) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
    publish({ status: 'ready', hasIdentity: identity.agentId.trim().length > 0 });
  } catch {
    // Transient: stay fail-closed (hidden) but schedule a bounded retry so a
    // real holder is not locked out for the whole session by a startup blip.
    publish({ status: 'ready', hasIdentity: false });
    const delay = RETRY_DELAYS_MS[attempt];
    if (delay !== undefined && !retryTimer) {
      retryTimer = setTimeout(() => {
        retryTimer = null;
        void attemptLoad(attempt + 1);
      }, delay);
    }
  } finally {
    inFlight = false;
  }
}

/** Kick a load if one isn't already settled, in flight, or scheduled. */
function ensureLoad() {
  if (!resolved && !inFlight && !retryTimer) void attemptLoad(0);
}

/** Test seam — clears the module cache so each test starts from `loading`. */
export function __resetTinyPlaceIdentityForTests() {
  cache = { status: 'loading', hasIdentity: false };
  resolved = false;
  inFlight = false;
  if (retryTimer) {
    clearTimeout(retryTimer);
    retryTimer = null;
  }
  listeners.clear();
}

export function useTinyPlaceIdentity(): TinyPlaceIdentityState {
  const [state, setState] = useState<TinyPlaceIdentityState>(cache);

  useEffect(() => {
    const sync = () => setState(cache);
    listeners.add(sync);
    ensureLoad();
    // If the first check failed and the user later unlocks their wallet, a
    // window refocus re-attempts immediately (cancelling any pending backoff) so
    // a real holder isn't hidden until restart.
    const onFocus = () => {
      if (resolved || inFlight) return;
      if (retryTimer) {
        clearTimeout(retryTimer);
        retryTimer = null;
      }
      void attemptLoad(0);
    };
    window.addEventListener('focus', onFocus);
    // Adopt the current cache in case it resolved between the initial render and
    // this effect firing (or a sibling hook already loaded it).
    sync();
    return () => {
      listeners.delete(sync);
      window.removeEventListener('focus', onFocus);
    };
  }, []);

  return state;
}
