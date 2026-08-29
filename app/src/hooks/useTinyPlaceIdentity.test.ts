import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { __resetTinyPlaceIdentityForTests, useTinyPlaceIdentity } from './useTinyPlaceIdentity';

const selfIdentity = vi.fn();
vi.mock('../lib/orchestration/orchestrationClient', () => ({
  orchestrationClient: { selfIdentity: () => selfIdentity() },
}));

// The wallet gate (#5805) runs before every identity fetch. Mock it here so the
// pre-existing cases below are deterministic rather than depending on a real
// `wallet_status` RPC failing into the `unknown` fall-through; `beforeEach`
// defaults it to a configured wallet, which is the path they were written for.
const fetchWalletStatus = vi.fn();
vi.mock('../services/walletApi', () => ({ fetchWalletStatus: () => fetchWalletStatus() }));

describe('useTinyPlaceIdentity (#5424)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    fetchWalletStatus.mockResolvedValue({ configured: true });
    __resetTinyPlaceIdentityForTests();
  });

  afterEach(() => {
    __resetTinyPlaceIdentityForTests();
    vi.useRealTimers();
  });

  // #5805 — `selfIdentity()` derives a tiny.place signer from the wallet, so a
  // wallet-less user could only ever get a rejection, which the retry ladder
  // then repeated: 55 error-level reports in 72 minutes on one session.
  // `wallet_status` answers the same question without erroring, so the call is
  // never made.
  it('never calls selfIdentity when no wallet is configured', async () => {
    fetchWalletStatus.mockResolvedValue({ configured: false });
    const { result } = renderHook(() => useTinyPlaceIdentity());

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(result.current.hasIdentity).toBe(false);
    expect(selfIdentity).not.toHaveBeenCalled();
  });

  // The gate fires only on a POSITIVE "no wallet". If `wallet_status` itself
  // fails we cannot prove the wallet is absent, so the call must still go out —
  // otherwise a transport blip would hide a real identity. The core boundary
  // classifier stays as defense-in-depth for whatever comes back.
  it('still calls selfIdentity when wallet status is inconclusive', async () => {
    fetchWalletStatus.mockRejectedValue(new Error('rpc transport failed'));
    selfIdentity.mockResolvedValue({ agentId: 'agent-123', handles: [], discoverable: true });
    const { result } = renderHook(() => useTinyPlaceIdentity());

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(selfIdentity).toHaveBeenCalledTimes(1);
    expect(result.current.hasIdentity).toBe(true);
  });

  it('reports an identity when the RPC returns a non-empty agentId', async () => {
    selfIdentity.mockResolvedValue({ agentId: 'agent-123', handles: [], discoverable: true });
    const { result } = renderHook(() => useTinyPlaceIdentity());

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(result.current.hasIdentity).toBe(true);
  });

  it('reports no identity when the agentId is blank', async () => {
    selfIdentity.mockResolvedValue({ agentId: '   ', handles: [], discoverable: false });
    const { result } = renderHook(() => useTinyPlaceIdentity());

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(result.current.hasIdentity).toBe(false);
  });

  it('stays fail-closed (hidden) while a rejected check is retried', async () => {
    selfIdentity.mockRejectedValue(new Error('wallet locked'));
    const { result } = renderHook(() => useTinyPlaceIdentity());

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(result.current.hasIdentity).toBe(false);
  });

  it('fetches once and shares the result across consumers', async () => {
    selfIdentity.mockResolvedValue({ agentId: 'agent-123', handles: [], discoverable: true });
    const first = renderHook(() => useTinyPlaceIdentity());
    const second = renderHook(() => useTinyPlaceIdentity());

    await waitFor(() => expect(first.result.current.status).toBe('ready'));
    expect(second.result.current.hasIdentity).toBe(true);
    // A single resolved RPC backs every consumer for the app session.
    expect(selfIdentity).toHaveBeenCalledTimes(1);
  });

  it('retries after a transient failure and recovers without a restart (#5439 review)', async () => {
    vi.useFakeTimers();
    selfIdentity
      .mockRejectedValueOnce(new Error('wallet locked at startup'))
      .mockResolvedValue({ agentId: 'agent-123', handles: [], discoverable: true });
    const { result } = renderHook(() => useTinyPlaceIdentity());

    // First attempt fails → fail-closed (hidden), but a backoff retry is queued.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(result.current).toEqual({ status: 'ready', hasIdentity: false });

    // The retry fires and succeeds → a real holder is no longer locked out.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(result.current.hasIdentity).toBe(true);
  });

  it('re-checks immediately on window focus after a failed startup check (#5439 review)', async () => {
    selfIdentity
      .mockRejectedValueOnce(new Error('wallet locked'))
      .mockResolvedValue({ agentId: 'agent-123', handles: [], discoverable: true });
    const { result } = renderHook(() => useTinyPlaceIdentity());

    await waitFor(() => expect(result.current.status).toBe('ready'));
    expect(result.current.hasIdentity).toBe(false);

    // The user unlocks their wallet and returns to the app.
    await act(async () => {
      window.dispatchEvent(new Event('focus'));
    });
    await waitFor(() => expect(result.current.hasIdentity).toBe(true));
  });
});
