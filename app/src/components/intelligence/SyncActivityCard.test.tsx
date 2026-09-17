/**
 * Vitest for `<SyncActivityCard />` (openhuman#6257): the sources with a sync
 * in flight on Brain › Sync, named and staged in plain language.
 *
 * The card reads the real sync store, fed here the way the socket feeds it
 * (`applyStageEvent`); only the status-list RPC is swapped for a spy.
 */
import { act, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { SourceStatus } from '../../services/memorySourcesService';
import {
  applyStageEvent,
  getMemorySyncActivity,
  noteSyncRequested,
  resetMemorySyncActivityForTests,
} from './memorySyncActivityStore';
import { STATUS_POLL_MS, SyncActivityCard } from './SyncActivityCard';

const mockStatusList = vi.fn();

vi.mock('../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (k: string) => k }) }));

vi.mock('../../services/memorySourcesService', async importOriginal => {
  const actual = await importOriginal<typeof import('../../services/memorySourcesService')>();
  return { ...actual, memorySourcesStatusList: (...args: unknown[]) => mockStatusList(...args) };
});

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

describe('<SyncActivityCard />', () => {
  beforeEach(() => {
    resetMemorySyncActivityForTests();
    mockStatusList.mockReset();
    mockStatusList.mockResolvedValue([]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('says nothing is syncing when no run is in flight', async () => {
    render(<SyncActivityCard />);
    expect(await screen.findByTestId('sync-activity-empty')).toHaveTextContent(
      'sync.nowSyncing.empty'
    );
  });

  it('lists a syncing source by its label with a plain-language stage', async () => {
    mockStatusList.mockResolvedValue([status('src-a', { label: 'Notes', sync_stage: null })]);
    render(<SyncActivityCard />);
    act(() => {
      applyStageEvent({
        stage: 'queued',
        source_id: 'src-a',
        detail: 'queued chunk extraction for mem_src:src-a:x.md',
      });
    });

    const row = await screen.findByTestId('sync-activity-row-src-a');
    await waitFor(() => expect(row).toHaveTextContent('Notes'));
    expect(row).toHaveTextContent('memorySources.stage.queued');
    expect(row).not.toHaveTextContent('mem_src:');
  });

  it('shows a progress detail a person can read, under the id when no label is known', async () => {
    mockStatusList.mockResolvedValue([status('src-b')]);
    render(<SyncActivityCard />);
    act(() => {
      applyStageEvent({
        stage: 'running',
        source_id: 'src-b',
        detail: 'pass 1 done, 200 item(s) so far',
      });
    });

    const row = await screen.findByTestId('sync-activity-row-src-b');
    expect(row).toHaveTextContent('src-b');
    expect(row).toHaveTextContent('memorySources.stage.running');
    expect(row).toHaveTextContent('pass 1 done, 200 item(s) so far');
  });

  it('shows a row the Sync button lit before any stage as starting', async () => {
    mockStatusList.mockResolvedValue([status('src-e')]);
    render(<SyncActivityCard />);
    act(() => {
      noteSyncRequested('src-e');
    });
    expect(await screen.findByTestId('sync-activity-row-src-e')).toHaveTextContent(
      'memorySources.stage.requested'
    );
  });

  it('seeds a run the core reports in flight when the card mounts', async () => {
    mockStatusList.mockResolvedValue([
      status('src-c', { label: 'Gmail', sync_stage: 'running', sync_detail: null }),
    ]);
    render(<SyncActivityCard />);

    const row = await screen.findByTestId('sync-activity-row-src-c');
    await waitFor(() => expect(row).toHaveTextContent('Gmail'));
    expect(getMemorySyncActivity().syncingIds.has('src-c')).toBe(true);
  });

  it('drops a row once its run ends', async () => {
    mockStatusList.mockResolvedValue([status('src-f')]);
    render(<SyncActivityCard />);
    act(() => {
      applyStageEvent({ stage: 'running', source_id: 'src-f', detail: null });
    });
    expect(await screen.findByTestId('sync-activity-row-src-f')).toBeInTheDocument();

    act(() => {
      applyStageEvent({ stage: 'completed', source_id: 'src-f', detail: 'ingested 1 item(s)' });
    });
    await waitFor(() =>
      expect(screen.queryByTestId('sync-activity-row-src-f')).not.toBeInTheDocument()
    );
    expect(screen.getByTestId('sync-activity-empty')).toBeInTheDocument();
  });

  it('polls the status list while mounted and stops on unmount', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const view = render(<SyncActivityCard />);
    await waitFor(() => expect(mockStatusList).toHaveBeenCalledTimes(1));

    await act(async () => {
      vi.advanceTimersByTime(STATUS_POLL_MS);
    });
    await waitFor(() => expect(mockStatusList).toHaveBeenCalledTimes(2));

    view.unmount();
    await act(async () => {
      vi.advanceTimersByTime(STATUS_POLL_MS * 2);
    });
    expect(mockStatusList).toHaveBeenCalledTimes(2);
  });

  it('keeps the last known sources when a later status read fails', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    mockStatusList
      .mockResolvedValueOnce([status('src-d', { label: 'Drive' })])
      .mockRejectedValue(new Error('core down'));
    render(<SyncActivityCard />);
    await waitFor(() => expect(mockStatusList).toHaveBeenCalledTimes(1));
    act(() => {
      applyStageEvent({ stage: 'running', source_id: 'src-d', detail: null });
    });
    const row = await screen.findByTestId('sync-activity-row-src-d');
    await waitFor(() => expect(row).toHaveTextContent('Drive'));

    await act(async () => {
      vi.advanceTimersByTime(STATUS_POLL_MS);
    });
    await waitFor(() => expect(warn).toHaveBeenCalled());
    expect(screen.getByTestId('sync-activity-row-src-d')).toHaveTextContent('Drive');
    warn.mockRestore();
  });

  it('lists only sources the registry knows, never a per-document row', async () => {
    mockStatusList.mockResolvedValue([status('src-g', { label: 'Gmail' })]);
    render(<SyncActivityCard />);
    await waitFor(() => expect(mockStatusList).toHaveBeenCalled());
    act(() => {
      // The bridge's per-document stage for a document outside the registry
      // arrives keyed by the document id, and no run ever ends it.
      applyStageEvent({
        stage: 'ingesting',
        connection_id: 'gmail:ca_1:msg-42',
        detail: 'queue_depth=3',
      });
      applyStageEvent({ stage: 'running', source_id: 'src-g', detail: null });
    });

    const row = await screen.findByTestId('sync-activity-row-src-g');
    await waitFor(() => expect(row).toHaveTextContent('Gmail'));
    expect(getMemorySyncActivity().syncingIds.has('gmail:ca_1:msg-42')).toBe(true);
    expect(screen.getByTestId('sync-activity-card')).not.toHaveTextContent('msg-42');
  });

  it('ignores a status read that answers after a newer one', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let answerFirst: (rows: SourceStatus[]) => void = () => {};
    mockStatusList
      .mockImplementationOnce(
        () =>
          new Promise<SourceStatus[]>(resolve => {
            answerFirst = resolve;
          })
      )
      .mockResolvedValue([status('src-h', { label: 'Newer', sync_stage: null })]);
    render(<SyncActivityCard />);
    await act(async () => {
      vi.advanceTimersByTime(STATUS_POLL_MS);
    });
    await waitFor(() => expect(mockStatusList).toHaveBeenCalledTimes(2));
    act(() => {
      applyStageEvent({ stage: 'running', source_id: 'src-h', detail: null });
    });
    const row = await screen.findByTestId('sync-activity-row-src-h');
    await waitFor(() => expect(row).toHaveTextContent('Newer'));

    // The first read answers last, with an older label.
    await act(async () => {
      answerFirst([status('src-h', { label: 'Older', sync_stage: null })]);
    });
    expect(screen.getByTestId('sync-activity-row-src-h')).toHaveTextContent('Newer');
  });

  it('hides the detail of a stage it does not know', async () => {
    mockStatusList.mockResolvedValue([status('src-u', { label: 'Feed' })]);
    render(<SyncActivityCard />);
    await waitFor(() => expect(mockStatusList).toHaveBeenCalled());
    act(() => {
      noteSyncRequested('src-u');
      applyStageEvent({
        stage: 'embedding',
        source_id: 'src-u',
        detail: 'batch for mem_src:src-u:7',
      });
    });

    const row = await screen.findByTestId('sync-activity-row-src-u');
    expect(row).toHaveTextContent('memorySources.stage.unknown');
    expect(row).not.toHaveTextContent('mem_src:');
  });
});
