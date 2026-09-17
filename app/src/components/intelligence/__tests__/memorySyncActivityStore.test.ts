/**
 * @vitest-environment jsdom
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { SourceStatus } from '../../../services/memorySourcesService';
import {
  applyStageEvent,
  getMemorySyncActivity,
  LATE_ITEM_STAGE_WINDOW_MS,
  noteSyncRejected,
  noteSyncRequested,
  RECONCILE_GRACE_MS,
  reconcileWithStatuses,
  resetMemorySyncActivityForTests,
  subscribeTerminalSyncEvents,
} from '../memorySyncActivityStore';

function status(source_id: string, extra: Partial<SourceStatus> = {}): SourceStatus {
  return {
    source_id,
    chunks_synced: 0,
    chunks_pending: 0,
    last_chunk_at_ms: null,
    freshness: 'idle',
    ...extra,
  };
}

describe('memorySyncActivityStore', () => {
  beforeEach(() => {
    resetMemorySyncActivityForTests();
  });

  it('keeps a live bar per row from the stage stream until the run ends', () => {
    applyStageEvent({ stage: 'running', source_id: 'src-a', detail: null });
    let s = getMemorySyncActivity();
    expect(s.progress.get('src-a')).toEqual({ stage: 'running', detail: null, percent: null });

    const ended = vi.fn();
    const unsubscribe = subscribeTerminalSyncEvents(ended);
    applyStageEvent({ stage: 'completed', source_id: 'src-a', detail: 'ingested 7 item(s)' });
    s = getMemorySyncActivity();
    expect(s.progress.has('src-a')).toBe(false);
    expect(s.syncingIds.has('src-a')).toBe(false);
    expect(s.results.get('src-a')).toEqual({ kind: 'success', items: 7, reason: null, note: null });
    expect(ended).toHaveBeenCalledWith(
      expect.objectContaining({
        rowId: 'src-a',
        stage: 'completed',
        result: { kind: 'success', items: 7, reason: null, note: null },
      })
    );
    unsubscribe();
  });

  it('is fed by the window event without any screen mounted', () => {
    window.dispatchEvent(
      new CustomEvent('openhuman:memory-sync-stage', {
        detail: { stage: 'fetching', source_id: 'src-b', detail: '2/4 pages' },
      })
    );
    const s = getMemorySyncActivity();
    expect(s.progress.get('src-b')).toEqual({
      stage: 'fetching',
      detail: '2/4 pages',
      percent: 50,
    });
    expect(s.syncingIds.has('src-b')).toBe(true);
  });

  it('lights the row on request and records a rejected request as a failed result', () => {
    applyStageEvent({ stage: 'completed', source_id: 'src-c', detail: 'ingested 1 item(s)' });
    noteSyncRequested('src-c');
    let s = getMemorySyncActivity();
    expect(s.syncingIds.has('src-c')).toBe(true);
    expect(s.results.has('src-c')).toBe(false);

    noteSyncRejected('src-c', 'transport down');
    s = getMemorySyncActivity();
    expect(s.syncingIds.has('src-c')).toBe(false);
    expect(s.results.get('src-c')).toEqual({
      kind: 'failed',
      items: null,
      reason: 'transport down',
      note: null,
    });
  });

  it('seeds a run the core reports that the store did not see start', () => {
    reconcileWithStatuses([status('src-d', { sync_stage: 'running', sync_detail: 'pass 2' })]);
    const s = getMemorySyncActivity();
    expect(s.progress.get('src-d')).toEqual({ stage: 'running', detail: 'pass 2', percent: null });
    // The flag follows the bar on a cold mount: the button must read as
    // syncing, not only the row.
    expect(s.syncingIds.has('src-d')).toBe(true);
  });

  it("marks the connector's `running` stage as syncing like the reader stages", () => {
    applyStageEvent({ stage: 'running', source_id: 'src-h', detail: null });
    expect(getMemorySyncActivity().syncingIds.has('src-h')).toBe(true);
  });

  it('leaves a fresh local entry alone when the core says idle, and clears a stale one', () => {
    const t0 = 1_000_000;
    applyStageEvent({ stage: 'running', source_id: 'src-e', detail: null });
    // The store stamps Date.now(); reconcile with a "now" inside the grace.
    reconcileWithStatuses([status('src-e', { sync_stage: null, sync_detail: null })], Date.now());
    expect(getMemorySyncActivity().progress.has('src-e')).toBe(true);

    reconcileWithStatuses(
      [status('src-e', { sync_stage: null, sync_detail: null })],
      Date.now() + RECONCILE_GRACE_MS + t0
    );
    expect(getMemorySyncActivity().progress.has('src-e')).toBe(false);
    expect(getMemorySyncActivity().syncingIds.has('src-e')).toBe(false);
  });

  it('seeds the bar for a row the button lit when the poll reports it live', () => {
    // The optimistic flag has no stage; if the first socket event was missed,
    // the poll carries it.
    noteSyncRequested('src-g');
    reconcileWithStatuses([status('src-g', { sync_stage: 'running', sync_detail: null })]);
    const s = getMemorySyncActivity();
    expect(s.progress.get('src-g')).toEqual({ stage: 'running', detail: null, percent: null });
    expect(s.syncingIds.has('src-g')).toBe(true);
  });

  it('changes nothing for a core that does not report the field', () => {
    applyStageEvent({ stage: 'running', source_id: 'src-f', detail: null });
    reconcileWithStatuses([status('src-f')], Date.now() + RECONCILE_GRACE_MS * 10);
    expect(getMemorySyncActivity().progress.has('src-f')).toBe(true);
  });

  // openhuman#6257: the core's bridge re-emits per-document stages, and they
  // can trail the run's terminal event.
  it('keeps the result when a per-item stage trails the terminal event', () => {
    applyStageEvent({ stage: 'running', source_id: 'src-i', detail: null });
    applyStageEvent({ stage: 'completed', source_id: 'src-i', detail: 'ingested 2 item(s)' });
    for (const stage of ['stored', 'queued', 'ingesting']) {
      applyStageEvent({ stage, source_id: 'src-i', detail: 'queued chunk extraction' });
    }
    const s = getMemorySyncActivity();
    expect(s.progress.has('src-i')).toBe(false);
    expect(s.syncingIds.has('src-i')).toBe(false);
    expect(s.results.get('src-i')).toEqual({ kind: 'success', items: 2, reason: null, note: null });
  });

  it('tracks a new run after a finished one', () => {
    applyStageEvent({ stage: 'failed', source_id: 'src-j', detail: 'boom' });
    applyStageEvent({ stage: 'requested', source_id: 'src-j', detail: null });
    applyStageEvent({ stage: 'queued', source_id: 'src-j', detail: null });
    const s = getMemorySyncActivity();
    expect(s.progress.get('src-j')?.stage).toBe('queued');
    expect(s.syncingIds.has('src-j')).toBe(true);
    expect(s.results.has('src-j')).toBe(false);
  });

  it('lets a Sync press start a new run whose item stages count again', () => {
    applyStageEvent({ stage: 'completed', source_id: 'src-k', detail: 'ingested 0 item(s)' });
    noteSyncRequested('src-k');
    applyStageEvent({ stage: 'stored', source_id: 'src-k', detail: null });
    expect(getMemorySyncActivity().progress.get('src-k')?.stage).toBe('stored');
  });

  it('does not seed a per-item stage the poll reports for a finished run', () => {
    applyStageEvent({ stage: 'completed', source_id: 'src-l', detail: 'ingested 1 item(s)' });
    reconcileWithStatuses([status('src-l', { sync_stage: 'queued', sync_detail: null })]);
    const s = getMemorySyncActivity();
    expect(s.progress.has('src-l')).toBe(false);
    expect(s.syncingIds.has('src-l')).toBe(false);
  });

  // A run whose start the app never saw shows up as per-item stages only, so
  // once the window the core applies too has passed, those stages count.
  it('counts per-item stages again once the finished run is past the window', () => {
    const now = vi.spyOn(Date, 'now');
    const finishedAt = 5_000_000;
    now.mockReturnValue(finishedAt);
    applyStageEvent({ stage: 'completed', source_id: 'src-m', detail: 'ingested 1 item(s)' });

    now.mockReturnValue(finishedAt + LATE_ITEM_STAGE_WINDOW_MS);
    applyStageEvent({ stage: 'queued', source_id: 'src-m', detail: null });
    expect(getMemorySyncActivity().progress.has('src-m')).toBe(false);

    now.mockReturnValue(finishedAt + LATE_ITEM_STAGE_WINDOW_MS + 1);
    applyStageEvent({ stage: 'queued', source_id: 'src-m', detail: null });
    expect(getMemorySyncActivity().progress.get('src-m')?.stage).toBe('queued');
    expect(getMemorySyncActivity().syncingIds.has('src-m')).toBe(true);
    now.mockRestore();
  });

  it('lets the poll seed a per-item stage once the finished run is past the window', () => {
    const finishedAt = 7_000_000;
    const now = vi.spyOn(Date, 'now').mockReturnValue(finishedAt);
    applyStageEvent({ stage: 'completed', source_id: 'src-n', detail: 'ingested 1 item(s)' });
    now.mockRestore();

    const live = [status('src-n', { sync_stage: 'ingesting', sync_detail: null })];
    reconcileWithStatuses(live, finishedAt + LATE_ITEM_STAGE_WINDOW_MS);
    expect(getMemorySyncActivity().syncingIds.has('src-n')).toBe(false);
    reconcileWithStatuses(live, finishedAt + LATE_ITEM_STAGE_WINDOW_MS + 1);
    expect(getMemorySyncActivity().syncingIds.has('src-n')).toBe(true);
  });
});
