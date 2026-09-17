/**
 * Vitest for `<SyncAuditPanel />` (issue #3116 — coverage for the sync-audit
 * history surface shipped in PR #3113; openhuman#6257 — the panel keeps
 * itself current).
 *
 * Covers:
 * - loading → loaded transition (renders rows once the audit log resolves)
 * - formatting of tokens (k/M), cost ($x.xxxx), and duration (ms / s / m s)
 * - the scope label mapping for github / gmail / rebuild scopes
 * - success ✓ vs failure ✗ status glyphs
 * - the empty state when no runs are recorded
 * - registry labels from the status list, with the scope label as fallback
 * - re-reading after a run ends, polling while one runs, and manual Refresh
 * - an older read that answers last never replacing a newer history
 *
 * Only the `memorySyncAuditLog` and `memorySourcesStatusList` wrappers are
 * swapped for spies; everything else in those modules is inherited verbatim.
 * The sync store is the real one, fed through `applyStageEvent` the way the
 * socket feeds it.
 */
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { SourceStatus } from '../../services/memorySourcesService';
import type { SyncAuditEntry } from '../../utils/tauriCommands';
import { applyStageEvent, resetMemorySyncActivityForTests } from './memorySyncActivityStore';
import {
  POLL_WHILE_SYNCING_MS,
  REFETCH_AFTER_RUN_ENDS_MS,
  SyncAuditPanel,
  timeAgo,
} from './SyncAuditPanel';

const mockAuditLog = vi.fn();
const mockStatusList = vi.fn();

vi.mock('../../utils/tauriCommands', async importOriginal => {
  const actual = await importOriginal<typeof import('../../utils/tauriCommands')>();
  return { ...actual, memorySyncAuditLog: (...args: unknown[]) => mockAuditLog(...args) };
});

vi.mock('../../services/memorySourcesService', async importOriginal => {
  const actual = await importOriginal<typeof import('../../services/memorySourcesService')>();
  return { ...actual, memorySourcesStatusList: (...args: unknown[]) => mockStatusList(...args) };
});

function entry(overrides: Partial<SyncAuditEntry> = {}): SyncAuditEntry {
  return {
    timestamp: new Date().toISOString(),
    source_id: 'src-1',
    source_kind: 'github_repo',
    scope: 'github:tinyhumansai/openhuman',
    items_fetched: 12,
    batches: 1,
    input_tokens: 1500,
    output_tokens: 500,
    estimated_cost_usd: 0.0123,
    duration_ms: 4200,
    success: true,
    ...overrides,
  };
}

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

describe('<SyncAuditPanel />', () => {
  beforeEach(() => {
    resetMemorySyncActivityForTests();
    mockAuditLog.mockReset();
    mockStatusList.mockReset();
    mockStatusList.mockResolvedValue([]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders the empty state when there are no runs', async () => {
    mockAuditLog.mockResolvedValue([]);
    render(<SyncAuditPanel />);
    expect(await screen.findByText('No sync runs recorded yet.')).toBeInTheDocument();
  });

  it('renders entries once the audit log resolves', async () => {
    mockAuditLog.mockResolvedValue([entry()]);
    render(<SyncAuditPanel />);

    // Summary line: "1 sync runs" (count + label render as sibling text nodes
    // inside one span, so match the span's normalized text content).
    expect(await screen.findByText(/^\s*1\s+sync runs\s*$/)).toBeInTheDocument();
    // The github scope is rendered through scopeLabel().
    expect(screen.getByText('GitHub · tinyhumansai/openhuman')).toBeInTheDocument();
    // Item count cell.
    expect(screen.getByText('12')).toBeInTheDocument();
  });

  it('formats tokens, cost, and duration for each row', async () => {
    mockAuditLog.mockResolvedValue([
      entry({
        input_tokens: 1_500, // 1.5k in
        output_tokens: 500, // combined 2.0k displayed
        estimated_cost_usd: 0.0123,
        duration_ms: 4200, // 4.2s
      }),
    ]);
    render(<SyncAuditPanel />);

    // formatTokens(input+output) → 2.0k
    expect(await screen.findByText('2.0k')).toBeInTheDocument();
    // cost → $0.0123 (4 dp)
    expect(screen.getByText('$0.0123')).toBeInTheDocument();
    // duration → 4.2s
    expect(screen.getByText('4.2s')).toBeInTheDocument();
  });

  it('formats sub-second, minute, and millions correctly', async () => {
    mockAuditLog.mockResolvedValue([
      entry({
        source_id: 'big',
        scope: 'gmail:test-at-example-dot-com',
        input_tokens: 1_500_000, // 1.5M in
        output_tokens: 500_000, // combined 2.0M
        duration_ms: 125_000, // 2m 5s
        estimated_cost_usd: 1.5,
      }),
    ]);
    render(<SyncAuditPanel />);

    expect(await screen.findByText('2.0M')).toBeInTheDocument();
    expect(screen.getByText('2m 5s')).toBeInTheDocument();
    // gmail scope label de-slugifies the email.
    expect(screen.getByText('Gmail · test@example.com')).toBeInTheDocument();
  });

  it('aggregates totals across multiple runs', async () => {
    mockAuditLog.mockResolvedValue([
      entry({ source_id: 'a', estimated_cost_usd: 0.01, input_tokens: 1000, output_tokens: 0 }),
      entry({ source_id: 'b', estimated_cost_usd: 0.02, input_tokens: 1000, output_tokens: 0 }),
    ]);
    render(<SyncAuditPanel />);

    // "2 sync runs"
    expect(await screen.findByText(/^\s*2\s+sync runs\s*$/)).toBeInTheDocument();
    // Total cost = 0.03 rendered with the "total" suffix in the summary span.
    expect(screen.getByText(/\$0\.0300\s+total/)).toBeInTheDocument();
  });

  it('renders the failure glyph for unsuccessful runs', async () => {
    mockAuditLog.mockResolvedValue([
      entry({ success: false, error: 'rate limited', source_id: 'fail' }),
    ]);
    render(<SyncAuditPanel />);

    const failGlyph = await screen.findByTitle('rate limited');
    expect(failGlyph).toHaveTextContent('✗');
  });

  it('renders the partial glyph when the fetch succeeded but tree ingest failed', async () => {
    // openhuman#5820: fetched items with a failed memory-tree half must not
    // read as plain failure (nothing fetched) and MUST not read as success.
    mockAuditLog.mockResolvedValue([
      entry({
        success: false,
        items_fetched: 250,
        tree_ingest_failures: 250,
        tree_error: '250 item(s) fetched but not ingested into the memory tree',
        source_id: 'partial',
      }),
    ]);
    render(<SyncAuditPanel />);

    const partialGlyph = await screen.findByTitle(
      '250 item(s) fetched but not ingested into the memory tree'
    );
    expect(partialGlyph).toHaveTextContent('⚠');
  });

  it('falls back to the partial label when the row has no tree_error text', async () => {
    mockAuditLog.mockResolvedValue([
      entry({ success: false, tree_ingest_failures: 3, source_id: 'partial-no-text' }),
    ]);
    render(<SyncAuditPanel />);

    const partialGlyph = await screen.findByTitle('Fetched, memory ingest failed');
    expect(partialGlyph).toHaveTextContent('⚠');
  });

  it('renders the success glyph for successful runs', async () => {
    mockAuditLog.mockResolvedValue([entry({ success: true })]);
    render(<SyncAuditPanel />);

    const okGlyph = await screen.findByTitle('Success');
    expect(okGlyph).toHaveTextContent('✓');
  });

  it('maps a rebuild scope through its label', async () => {
    mockAuditLog.mockResolvedValue([
      entry({ source_kind: 'rebuild', scope: 'rebuild:gmail:x-at-y-dot-com', source_id: 'r' }),
    ]);
    render(<SyncAuditPanel />);

    expect(await screen.findByText('Rebuild · gmail:x-at-y-dot-com')).toBeInTheDocument();
  });

  it('survives a fetch failure by leaving the empty state', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    mockAuditLog.mockRejectedValue(new Error('boom'));
    render(<SyncAuditPanel />);

    // After the rejected fetch, loading clears and the empty state shows.
    await waitFor(() => expect(screen.getByText('No sync runs recorded yet.')).toBeInTheDocument());
    expect(consoleError).toHaveBeenCalled();
    consoleError.mockRestore();
  });

  it('renders a full table with headers when entries exist', async () => {
    mockAuditLog.mockResolvedValue([entry()]);
    render(<SyncAuditPanel />);

    const table = await screen.findByRole('table');
    expect(within(table).getByText('When')).toBeInTheDocument();
    expect(within(table).getByText('Source')).toBeInTheDocument();
    expect(within(table).getByText('Cost')).toBeInTheDocument();
  });

  it('names a row by its registry label and keeps the scope as the title', async () => {
    mockAuditLog.mockResolvedValue([
      entry({ source_id: 'src-gmail', source_kind: 'composio', scope: 'gmail:ca_1' }),
    ]);
    mockStatusList.mockResolvedValue([status('src-gmail', { label: 'Gmail · work' })]);
    render(<SyncAuditPanel />);

    const cell = await screen.findByText('Gmail · work');
    expect(cell).toHaveAttribute('title', 'gmail:ca_1');
  });

  it('falls back to the scope label when the status list fails', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    mockAuditLog.mockResolvedValue([entry()]);
    mockStatusList.mockRejectedValue(new Error('core down'));
    render(<SyncAuditPanel />);

    expect(await screen.findByText('GitHub · tinyhumansai/openhuman')).toBeInTheDocument();
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it('re-reads the history shortly after a sync run ends', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mockAuditLog
      .mockResolvedValueOnce([])
      .mockResolvedValue([entry({ source_id: 'src-new', scope: 'github:org/new' })]);
    render(<SyncAuditPanel />);
    await screen.findByText('No sync runs recorded yet.');

    act(() => {
      applyStageEvent({ stage: 'completed', source_id: 'src-new', detail: 'ingested 3 item(s)' });
    });
    expect(mockAuditLog).toHaveBeenCalledTimes(1);

    await act(async () => {
      vi.advanceTimersByTime(REFETCH_AFTER_RUN_ENDS_MS);
    });
    expect(await screen.findByText('GitHub · org/new')).toBeInTheDocument();
    expect(mockAuditLog).toHaveBeenCalledTimes(2);
  });

  it('polls while a sync is running and stops once it ends', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mockAuditLog.mockResolvedValue([]);
    mockStatusList.mockResolvedValue([status('src-live')]);
    render(<SyncAuditPanel />);
    await screen.findByText('No sync runs recorded yet.');
    expect(mockAuditLog).toHaveBeenCalledTimes(1);

    act(() => {
      applyStageEvent({ stage: 'running', source_id: 'src-live', detail: null });
    });
    await act(async () => {
      vi.advanceTimersByTime(POLL_WHILE_SYNCING_MS);
    });
    await waitFor(() => expect(mockAuditLog).toHaveBeenCalledTimes(2));

    act(() => {
      applyStageEvent({ stage: 'completed', source_id: 'src-live', detail: 'ingested 1 item(s)' });
    });
    await act(async () => {
      vi.advanceTimersByTime(REFETCH_AFTER_RUN_ENDS_MS);
    });
    await waitFor(() => expect(mockAuditLog).toHaveBeenCalledTimes(3));

    await act(async () => {
      vi.advanceTimersByTime(POLL_WHILE_SYNCING_MS * 2);
    });
    expect(mockAuditLog).toHaveBeenCalledTimes(3);
  });

  it('does not keep polling for a per-document row the registry does not know', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mockAuditLog.mockResolvedValue([]);
    mockStatusList.mockResolvedValue([status('src-known')]);
    render(<SyncAuditPanel />);
    await screen.findByText('No sync runs recorded yet.');

    act(() => {
      applyStageEvent({
        stage: 'ingesting',
        connection_id: 'slack:workspace-1:msg',
        detail: 'queue_depth=1',
      });
    });
    await act(async () => {
      vi.advanceTimersByTime(POLL_WHILE_SYNCING_MS * 2);
    });
    expect(mockAuditLog).toHaveBeenCalledTimes(1);
  });

  it('shows the history without waiting for the status list', async () => {
    mockAuditLog.mockResolvedValue([entry({ source_id: 'src-slow', scope: 'github:org/slow' })]);
    mockStatusList.mockImplementation(() => new Promise<SourceStatus[]>(() => {}));
    render(<SyncAuditPanel />);

    expect(await screen.findByText('GitHub · org/slow')).toBeInTheDocument();
    expect(screen.getByTestId('sync-history-refresh')).not.toBeDisabled();
  });

  it('re-reads the history when Refresh is pressed', async () => {
    mockAuditLog.mockResolvedValue([]);
    render(<SyncAuditPanel />);

    fireEvent.click(await screen.findByTestId('sync-history-refresh'));
    await waitFor(() => expect(mockAuditLog).toHaveBeenCalledTimes(2));
  });

  it('keeps the newer history when an older read answers last', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let resolveFirst: (rows: SyncAuditEntry[]) => void = () => {};
    mockAuditLog
      .mockImplementationOnce(
        () =>
          new Promise<SyncAuditEntry[]>(resolve => {
            resolveFirst = resolve;
          })
      )
      .mockResolvedValueOnce([entry({ source_id: 'newer', scope: 'github:org/newer' })]);
    render(<SyncAuditPanel />);

    act(() => {
      applyStageEvent({ stage: 'completed', source_id: 'newer', detail: 'ingested 1 item(s)' });
    });
    await act(async () => {
      vi.advanceTimersByTime(REFETCH_AFTER_RUN_ENDS_MS);
    });
    expect(await screen.findByText('GitHub · org/newer')).toBeInTheDocument();

    await act(async () => {
      resolveFirst([entry({ source_id: 'older', scope: 'github:org/older' })]);
    });
    expect(screen.queryByText('GitHub · org/older')).not.toBeInTheDocument();
    expect(screen.getByText('GitHub · org/newer')).toBeInTheDocument();
  });
});

describe('timeAgo', () => {
  // Resolve to the fallback English so the `{n}` interpolation is exercised.
  const t = (_key: string, fallback?: string) => fallback ?? _key;
  const isoAgo = (ms: number) => new Date(Date.now() - ms).toISOString();

  it('covers every relative-time bucket and substitutes the {n} placeholder', () => {
    expect(timeAgo(isoAgo(0), t)).toBe('just now');
    expect(timeAgo(isoAgo(5 * 60_000), t)).toBe('5m ago');
    expect(timeAgo(isoAgo(3 * 60 * 60_000), t)).toBe('3h ago');
    expect(timeAgo(isoAgo(2 * 24 * 60 * 60_000), t)).toBe('2d ago');
  });
});
