import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { PendingApproval } from '../../services/api/approvalApi';
import {
  getFlowPendingApprovalsSnapshot,
  refreshFlowPendingApprovals,
  resetFlowPendingApprovalsStoreForTests,
  retainFlowPendingApprovalsPolling,
  useFlowPendingApprovalsSource,
} from '../flowPendingApprovalsStore';

const fetchPendingApprovals = vi.hoisted(() => vi.fn());
const debugLog = vi.hoisted(() => vi.fn());
vi.mock('../../services/api/approvalApi', () => ({ fetchPendingApprovals }));
vi.mock('debug', () => ({ default: () => debugLog }));

function makeApproval(overrides: Partial<PendingApproval> = {}): PendingApproval {
  return {
    request_id: 'req-1',
    tool_name: 'shell',
    action_summary: 'Run a private command',
    args_redacted: {},
    session_id: 'session-1',
    created_at: '2026-01-01T00:00:00Z',
    expires_at: null,
    source_context: { kind: 'flow', flow_id: 'flow-1', run_id: 'run-1' },
    ...overrides,
  };
}

describe('flowPendingApprovalsStore', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    resetFlowPendingApprovalsStoreForTests();
  });

  afterEach(() => {
    resetFlowPendingApprovalsStoreForTests();
    vi.useRealTimers();
  });

  it('shares one immediate request and one timer across concurrent enabled consumers', async () => {
    fetchPendingApprovals.mockResolvedValue([makeApproval()]);

    const first = renderHook(() => useFlowPendingApprovalsSource(true));
    const second = renderHook(() => useFlowPendingApprovalsSource(true));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(fetchPendingApprovals).toHaveBeenCalledTimes(1);
    expect(first.result.current.polling).toBe(true);
    expect(second.result.current.approvals).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(1);

    first.unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(fetchPendingApprovals).toHaveBeenCalledTimes(2);

    second.unmount();
    expect(getFlowPendingApprovalsSnapshot().polling).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('returns stable frozen snapshots and does not expose the fetched array for mutation', async () => {
    const fetched = [makeApproval()];
    fetchPendingApprovals.mockResolvedValue(fetched);

    const initial = getFlowPendingApprovalsSnapshot();
    expect(getFlowPendingApprovalsSnapshot()).toBe(initial);
    expect(Object.isFrozen(initial)).toBe(true);
    expect(Object.isFrozen(initial.approvals)).toBe(true);

    await refreshFlowPendingApprovals();
    const successful = getFlowPendingApprovalsSnapshot();
    expect(successful).toBe(getFlowPendingApprovalsSnapshot());
    expect(successful.approvals).not.toBe(fetched);
    expect(Object.isFrozen(successful)).toBe(true);
    expect(Object.isFrozen(successful.approvals)).toBe(true);
  });

  it('keeps the last good approvals, normalizes an error, and retries on the next tick', async () => {
    fetchPendingApprovals
      .mockResolvedValueOnce([makeApproval()])
      .mockRejectedValueOnce(new Error('temporary transport failure'))
      .mockResolvedValueOnce([]);
    const release = retainFlowPendingApprovalsPolling();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(getFlowPendingApprovalsSnapshot().approvals).toHaveLength(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(getFlowPendingApprovalsSnapshot()).toMatchObject({
      approvals: [expect.objectContaining({ request_id: 'req-1' })],
      error: 'temporary transport failure',
      polling: true,
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(getFlowPendingApprovalsSnapshot()).toMatchObject({
      approvals: [],
      error: null,
      polling: true,
    });
    release();
  });

  it('invalidates an in-flight request and clears its timer on final release', async () => {
    let resolveRequest!: (approvals: PendingApproval[]) => void;
    fetchPendingApprovals.mockReturnValue(
      new Promise<PendingApproval[]>(resolve => {
        resolveRequest = resolve;
      })
    );
    const release = retainFlowPendingApprovalsPolling();
    expect(fetchPendingApprovals).toHaveBeenCalledTimes(1);

    release();
    resolveRequest([makeApproval()]);
    await act(async () => {
      await Promise.resolve();
    });

    expect(getFlowPendingApprovalsSnapshot()).toMatchObject({ approvals: [], polling: false });
    expect(vi.getTimerCount()).toBe(0);
  });

  it('logs only safe failure metadata, without approval payloads or error text', async () => {
    fetchPendingApprovals
      .mockResolvedValueOnce([makeApproval({ action_summary: 'private user-authored text' })])
      .mockRejectedValueOnce(new Error('private transport error'));
    const release = retainFlowPendingApprovalsPolling();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });

    const logged = JSON.stringify(debugLog.mock.calls);
    expect(logged).not.toContain('private user-authored text');
    expect(logged).not.toContain('private transport error');
    release();
  });
});
