import debug from 'debug';
import { useEffect, useSyncExternalStore } from 'react';

import { fetchPendingApprovals, type PendingApproval } from '../services/api/approvalApi';

const log = debug('flows:pending-approvals-store');
const POLL_INTERVAL_MS = 2000;

export interface FlowPendingApprovalsSnapshot {
  approvals: PendingApproval[];
  error: string | null;
  polling: boolean;
}

const freezeApprovals = (approvals: PendingApproval[]): PendingApproval[] =>
  Object.freeze([...approvals]) as PendingApproval[];

const makeSnapshot = (
  approvals: PendingApproval[],
  error: string | null,
  polling: boolean
): FlowPendingApprovalsSnapshot =>
  Object.freeze({ approvals: freezeApprovals(approvals), error, polling });

const INITIAL_SNAPSHOT = makeSnapshot([], null, false);

let snapshot = INITIAL_SNAPSHOT;
let retainCount = 0;
let pollTimer: number | undefined;
let requestGeneration = 0;
let nextRequestId = 0;
let activeRequestId: number | null = null;
let inFlight: Promise<void> | null = null;
const listeners = new Set<() => void>();

function emit(next: FlowPendingApprovalsSnapshot): void {
  snapshot = next;
  for (const listener of listeners) listener();
}

function normalizeError(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  const message = String(error);
  return message.trim() ? message : 'Unable to load pending approvals';
}

function clearPollTimer(): void {
  if (pollTimer === undefined) return;
  window.clearTimeout(pollTimer);
  pollTimer = undefined;
}

function scheduleNextPoll(generation: number): void {
  if (retainCount === 0 || generation !== requestGeneration || pollTimer !== undefined) return;
  pollTimer = window.setTimeout(() => {
    pollTimer = undefined;
    void refreshFlowPendingApprovals();
  }, POLL_INTERVAL_MS);
}

export function subscribeFlowPendingApprovals(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getFlowPendingApprovalsSnapshot(): FlowPendingApprovalsSnapshot {
  return snapshot;
}

export function refreshFlowPendingApprovals(): Promise<void> {
  if (inFlight) return inFlight;

  clearPollTimer();
  const generation = requestGeneration;
  const requestId = ++nextRequestId;
  activeRequestId = requestId;
  const request = (async () => {
    try {
      const approvals = await fetchPendingApprovals();
      if (generation !== requestGeneration) return;
      emit(makeSnapshot(approvals, null, retainCount > 0));
      log('refresh succeeded approval_count=%d', approvals.length);
    } catch (error) {
      if (generation !== requestGeneration) return;
      emit(makeSnapshot(snapshot.approvals, normalizeError(error), retainCount > 0));
      const errorType = error instanceof Error ? error.name : typeof error;
      log('refresh failed error_type=%s', errorType);
    } finally {
      if (activeRequestId === requestId) {
        activeRequestId = null;
        inFlight = null;
      }
      scheduleNextPoll(generation);
    }
  })();
  inFlight = request;
  return request;
}

export function retainFlowPendingApprovalsPolling(): () => void {
  retainCount += 1;
  if (retainCount === 1) {
    emit(makeSnapshot(snapshot.approvals, snapshot.error, true));
    void refreshFlowPendingApprovals();
  }

  let released = false;
  return () => {
    if (released) return;
    released = true;
    retainCount = Math.max(0, retainCount - 1);
    if (retainCount > 0) return;

    clearPollTimer();
    requestGeneration += 1;
    activeRequestId = null;
    inFlight = null;
    emit(makeSnapshot(snapshot.approvals, snapshot.error, false));
  };
}

export function useFlowPendingApprovalsSource(enabled: boolean): FlowPendingApprovalsSnapshot {
  const current = useSyncExternalStore(
    subscribeFlowPendingApprovals,
    getFlowPendingApprovalsSnapshot,
    getFlowPendingApprovalsSnapshot
  );

  useEffect(() => {
    if (!enabled) return;
    return retainFlowPendingApprovalsPolling();
  }, [enabled]);

  return current;
}

export function resetFlowPendingApprovalsStoreForTests(): void {
  clearPollTimer();
  requestGeneration += 1;
  activeRequestId = null;
  retainCount = 0;
  inFlight = null;
  snapshot = INITIAL_SNAPSHOT;
  listeners.clear();
}
