import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit';
import debug from 'debug';

import { callCoreRpc } from '../services/coreRpcClient';
import { CORE_RPC_METHODS } from '../services/rpcMethods';
import { resetUserScopedState } from './resetActions';

const privacyLog = debug('privacy:slice');

/**
 * Privacy Mode values as serialized by the Rust core (snake_case). Mirrors the
 * `PrivacyMode` type in {@link ../components/settings/panels/PrivacyModeSection}.
 * The status pill reads this to show the *current posture* alongside the live
 * per-thread external-transfer flag — it does NOT own the setting (that stays in
 * PrivacyModeSection); the slice is hydrated on boot and kept loosely in sync.
 */
export type PrivacyMode = 'local_only' | 'standard' | 'sensitive';

interface PrivacyState {
  /**
   * Current data-egress posture. `null` until hydrated from the core (or if the
   * RPC fails). Kept for the persistent status pill; not authoritative — the
   * setting lives in the core and is edited via PrivacyModeSection.
   */
  privacyMode: PrivacyMode | null;
  /**
   * Per-thread "an external transfer is active on the current turn" flag. Set
   * true when an `external_transfer_pending` event arrives and CLEARED on the
   * turn boundary (chat_done / chat_error / socket-disconnect reconcile) by
   * ChatRuntimeProvider. This is the SOLE source of truth for the status pill's
   * off-device sub-state.
   */
  activeExternalByThread: Record<string, boolean>;
}

const initialState: PrivacyState = { privacyMode: null, activeExternalByThread: {} };

/**
 * Hydrate the current Privacy Mode from the core on boot. Mirrors the RPC
 * PrivacyModeSection uses (`config_get_privacy_mode`) whose result is the
 * double-wrapped `{ result: { mode } }` shape. Failures resolve to `null` so
 * the pill degrades gracefully rather than throwing.
 */
export const hydratePrivacyMode = createAsyncThunk<PrivacyMode | null>(
  'privacy/hydratePrivacyMode',
  async () => {
    try {
      const resp = await callCoreRpc<{ result: { mode: PrivacyMode } }>({
        method: CORE_RPC_METHODS.configGetPrivacyMode,
        params: {},
      });
      privacyLog('[privacy] hydrated mode=%s', resp.result.mode);
      return resp.result.mode;
    } catch (err) {
      privacyLog('[privacy] failed to hydrate privacy mode: %o', err);
      return null;
    }
  }
);

const privacySlice = createSlice({
  name: 'privacy',
  initialState,
  reducers: {
    /** Set the current Privacy Mode (from boot hydration or a settings change). */
    setPrivacyMode: (state, action: PayloadAction<PrivacyMode>) => {
      privacyLog('[privacy] setPrivacyMode %s', action.payload);
      state.privacyMode = action.payload;
    },
    /**
     * Mark a thread as having a live external transfer so the status pill flips
     * off-device. Set when an `external_transfer_pending` event arrives; cleared
     * on the turn boundary by {@link clearActiveExternalForThread}.
     */
    markActiveExternalForThread: (state, action: PayloadAction<{ threadId: string }>) => {
      state.activeExternalByThread[action.payload.threadId] = true;
      privacyLog('[privacy] markActiveExternalForThread thread=%s', action.payload.threadId);
    },
    /**
     * Clear the live external-transfer flag for a thread. Dispatched by
     * ChatRuntimeProvider on the turn boundary (chat_done / chat_error /
     * disconnect reconcile) so the pill returns to on-device once the turn's
     * external activity is over.
     */
    clearActiveExternalForThread: (state, action: PayloadAction<{ threadId: string }>) => {
      if (state.activeExternalByThread[action.payload.threadId]) {
        delete state.activeExternalByThread[action.payload.threadId];
        privacyLog('[privacy] clearActiveExternalForThread thread=%s', action.payload.threadId);
      }
    },
  },
  extraReducers: builder => {
    // On identity flip / sign-out, drop per-user transfer flags. The privacy
    // mode is a core-side setting re-hydrated on the next boot, so it is safe to
    // reset here too.
    builder.addCase(resetUserScopedState, () => initialState);
    builder.addCase(hydratePrivacyMode.fulfilled, (state, action) => {
      if (action.payload) state.privacyMode = action.payload;
    });
  },
});

export const { setPrivacyMode, markActiveExternalForThread, clearActiveExternalForThread } =
  privacySlice.actions;

export default privacySlice.reducer;
