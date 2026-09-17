import { createSlice, type PayloadAction } from '@reduxjs/toolkit';

/**
 * Four independent connectivity channels surfaced separately so the UI can
 * tell the user *which* link is broken instead of one conflated "Disconnected"
 * pill (#1527).
 *
 * - `internet` — browser navigator.onLine. Source of truth: `online`/`offline`
 *   listeners on `window`.
 * - `core`     — local Rust sidecar reachability. Source: `coreHealthMonitor`
 *   poll of `openhuman.connectivity_diag`.
 * - `backend`  — the renderer's Socket.IO link to the local core, the realtime
 *   path for chat streaming and approvals. Source: `socketService` lifecycle
 *   callbacks.
 * - `hosted`   — the Rust core's own Socket.IO link to the hosted backend
 *   (webhooks, Composio triggers, managed-DM channel routing, hosted-brain
 *   runs). Source: `coreHealthMonitor` reading `socket_state` off the same
 *   `connectivity_diag` poll (#6256). `unknown` means the core's reconnect
 *   loop is not running at all — signed out, local session, early boot — and
 *   is deliberately not a degraded state: the core reports `reconnecting`
 *   between retries, so a link that is down but being retried is never
 *   mistaken for one that was never wanted.
 */

export type InternetState = 'online' | 'offline';
export type CoreState = 'reachable' | 'unreachable' | 'unknown';
export type BackendState = 'connected' | 'disconnected' | 'connecting';
/**
 * Mirrors the core's `ConnectionStatus` as reported by
 * `connectivity_diag.socket_state` while its reconnect loop is running
 * (`socket_loop_active`); `unknown` is the loop not running at all.
 *
 * - `connecting` / `reconnecting` — down and being retried (degraded).
 * - `disconnected` — the loop is alive but the server closed the Socket.IO
 *   namespace without closing the transport, so no events flow and no
 *   automatic reconnect happens (degraded). Never reported by a core that
 *   predates `socket_loop_active`: there it collapses to `unknown`.
 * - `error` — the server sent an `error` event on a transport that stays
 *   live and keeps emitting; not an outage, so it gets no chip until it has
 *   its own presentation. Stored so the state is observable.
 * - `stopped` — the loop exited for good because the stored session is
 *   unusable (no token, or the backend rejected it and nothing fresher
 *   existed): `socket_loop_stopped_on_failure`. Integrations and channels
 *   stay off until the user signs in again (degraded, with its own copy).
 */
export type HostedState =
  | 'connected'
  | 'connecting'
  | 'reconnecting'
  | 'disconnected'
  | 'error'
  | 'stopped'
  | 'unknown';

export interface ConnectivityState {
  internet: InternetState;
  core: CoreState;
  backend: BackendState;
  hosted: HostedState;
  /**
   * Last error string emitted per channel, if any. Cleared on the next
   * successful state for that channel. UI surfaces these in tooltips /
   * blocking screens for diagnosability.
   */
  lastError: { internet?: string; core?: string; backend?: string; hosted?: string };
}

const initialState: ConnectivityState = {
  internet: typeof navigator !== 'undefined' && navigator.onLine === false ? 'offline' : 'online',
  core: 'unknown',
  backend: 'connecting',
  hosted: 'unknown',
  lastError: {},
};

const slice = createSlice({
  name: 'connectivity',
  initialState,
  reducers: {
    setInternet(state, action: PayloadAction<{ value: InternetState; error?: string }>) {
      state.internet = action.payload.value;
      if (action.payload.value === 'online') {
        delete state.lastError.internet;
      } else {
        state.lastError.internet = action.payload.error;
      }
    },
    setCore(state, action: PayloadAction<{ value: CoreState; error?: string }>) {
      state.core = action.payload.value;
      if (action.payload.value === 'reachable') {
        delete state.lastError.core;
      } else {
        state.lastError.core = action.payload.error;
      }
    },
    setBackend(state, action: PayloadAction<{ value: BackendState; error?: string }>) {
      state.backend = action.payload.value;
      if (action.payload.value === 'connected' || action.payload.value === 'connecting') {
        // Clear the stale error on both successful connection and reconnect
        // attempts. Previously only 'connected' deleted lastError.backend,
        // which meant a prior disconnect error (e.g. "transport close") was
        // left set to `undefined` — not deleted — during 'connecting'. UI
        // components that read lastError.backend could still surface a stale
        // message in the reconnect window even though the key appeared falsy.
        delete state.lastError.backend;
      } else {
        state.lastError.backend = action.payload.error;
      }
    },
    setHosted(state, action: PayloadAction<{ value: HostedState; error?: string }>) {
      const { value, error } = action.payload;
      state.hosted = value;
      // `connected` and `unknown` are not outages, and a degraded reading that
      // carries no message must not leave an older one behind — delete rather
      // than assign `undefined` (same lesson as `setBackend` above).
      if (value === 'connected' || value === 'unknown' || error === undefined) {
        delete state.lastError.hosted;
      } else {
        state.lastError.hosted = error;
      }
    },
  },
});

export const { setInternet, setCore, setBackend, setHosted } = slice.actions;
export default slice.reducer;
