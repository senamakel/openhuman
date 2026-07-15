import { beforeEach, describe, expect, it, vi } from 'vitest';

import { callCoreRpc } from '../../services/coreRpcClient';
import privacyReducer, {
  clearActiveExternalForThread,
  hydratePrivacyMode,
  markActiveExternalForThread,
  setPrivacyMode,
} from '../privacySlice';
import { resetUserScopedState } from '../resetActions';

vi.mock('../../services/coreRpcClient', () => ({ callCoreRpc: vi.fn() }));

function initial() {
  return privacyReducer(undefined, { type: '@@INIT' });
}

describe('privacySlice — reducers', () => {
  it('has the expected initial state', () => {
    expect(initial()).toEqual({ privacyMode: null, activeExternalByThread: {} });
  });

  it('setPrivacyMode updates the mode', () => {
    const state = privacyReducer(initial(), setPrivacyMode('local_only'));
    expect(state.privacyMode).toBe('local_only');
  });

  it('resetUserScopedState wipes the mode and the active-external flags', () => {
    let state = privacyReducer(initial(), setPrivacyMode('standard'));
    state = privacyReducer(state, markActiveExternalForThread({ threadId: 't' }));
    expect(state.activeExternalByThread['t']).toBe(true);
    state = privacyReducer(state, resetUserScopedState());
    expect(state).toEqual({ privacyMode: null, activeExternalByThread: {} });
  });
});

describe('privacySlice — active external-transfer flag (status pill)', () => {
  it('markActiveExternalForThread marks the thread active-external', () => {
    const state = privacyReducer(initial(), markActiveExternalForThread({ threadId: 'thread-1' }));
    expect(state.activeExternalByThread['thread-1']).toBe(true);
  });

  it('clearActiveExternalForThread clears the flag', () => {
    let state = privacyReducer(initial(), markActiveExternalForThread({ threadId: 'thread-1' }));
    state = privacyReducer(state, clearActiveExternalForThread({ threadId: 'thread-1' }));
    expect(state.activeExternalByThread['thread-1']).toBeUndefined();
  });

  it('clearActiveExternalForThread is a no-op for an unknown thread', () => {
    const state = privacyReducer(initial(), clearActiveExternalForThread({ threadId: 'nope' }));
    expect(state.activeExternalByThread).toEqual({});
  });
});

describe('privacySlice — hydratePrivacyMode thunk', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('sets the mode from the double-wrapped RPC result on success', async () => {
    vi.mocked(callCoreRpc).mockResolvedValueOnce({ result: { mode: 'sensitive' } } as never);
    const action = await hydratePrivacyMode()(vi.fn(), vi.fn(), undefined);
    expect(action.payload).toBe('sensitive');

    const state = privacyReducer(initial(), action as never);
    expect(state.privacyMode).toBe('sensitive');
  });

  it('resolves to null (and leaves mode untouched) on RPC failure', async () => {
    vi.mocked(callCoreRpc).mockRejectedValueOnce(new Error('core down'));
    const action = await hydratePrivacyMode()(vi.fn(), vi.fn(), undefined);
    expect(action.payload).toBeNull();

    const seeded = privacyReducer(initial(), setPrivacyMode('standard'));
    const state = privacyReducer(seeded, action as never);
    // Null payload must not clobber an already-known mode.
    expect(state.privacyMode).toBe('standard');
  });
});
