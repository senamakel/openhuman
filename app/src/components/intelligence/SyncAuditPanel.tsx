/**
 * Sync audit history panel — shows when syncs happened, tokens consumed,
 * cost, and duration. Fetches from `openhuman.memory_sources_sync_audit_log`.
 *
 * Keeps itself current (openhuman#6257). It re-reads the history shortly after
 * any sync ends, polls while one is still running, and has a manual Refresh.
 * It used to fetch once on mount, so a run that finished while the tab was
 * open never appeared until the tab was left and re-entered.
 */
import { useCallback, useEffect, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { memorySourcesStatusList } from '../../services/memorySourcesService';
import { memorySyncAuditLog, type SyncAuditEntry } from '../../utils/tauriCommands';
import Button from '../ui/Button';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../ui/Table';
import { registrySyncingIds, sourceLabelsById } from './memorySourcesSyncTypes';
import { subscribeTerminalSyncEvents, useMemorySyncActivity } from './memorySyncActivityStore';

/**
 * How long after a run ends before the history is re-read. The core writes
 * the row for a run it drove before publishing the run's end, but the memory
 * driver's own periodic writer appends after its stage, so the read waits a
 * beat.
 */
export const REFETCH_AFTER_RUN_ENDS_MS = 1_000;

/** How often the history is re-read while any sync is still running. */
export const POLL_WHILE_SYNCING_MS = 10_000;

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const secs = ms / 1000;
  if (secs < 60) return `${secs.toFixed(1)}s`;
  const mins = Math.floor(secs / 60);
  const remSecs = Math.round(secs % 60);
  return `${mins}m ${remSecs}s`;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

function scopeLabel(scope: string): string {
  if (scope.startsWith('github:')) {
    return `GitHub · ${scope.slice(7)}`;
  }
  if (scope.startsWith('gmail:')) {
    return `Gmail · ${scope.slice(6).replace(/-at-/g, '@').replace(/-dot-/g, '.')}`;
  }
  if (scope.startsWith('rebuild:')) {
    return `Rebuild · ${scope.slice(8)}`;
  }
  return scope;
}

// `t` is threaded in because this is a module-level helper with no hook scope.
// The `{n}` placeholder follows the codebase's interpolation convention
// (t(...).replace('{n}', value)) — `t()` itself does not interpolate params.
export function timeAgo(iso: string, t: (key: string, fallback?: string) => string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60_000);
  if (mins < 1) return t('sync.timeAgo.justNow', 'just now');
  if (mins < 60) return t('sync.timeAgo.minutes', '{n}m ago').replace('{n}', String(mins));
  const hours = Math.floor(mins / 60);
  if (hours < 24) return t('sync.timeAgo.hours', '{n}h ago').replace('{n}', String(hours));
  const days = Math.floor(hours / 24);
  return t('sync.timeAgo.days', '{n}d ago').replace('{n}', String(days));
}

export function SyncAuditPanel() {
  const { t } = useT();
  const { syncingIds } = useMemorySyncActivity();
  const [entries, setEntries] = useState<SyncAuditEntry[]>([]);
  // Registry labels by source id. A row whose source has no label (removed
  // since, or a core that predates the field) keeps its scope label.
  const [labels, setLabels] = useState<Record<string, string>>({});
  // The registry's source ids. Only a run of one of them keeps the history
  // polling: the store also lights rows keyed by a document outside the
  // registry, and those never end (see `registrySyncingIds`).
  const [sourceIds, setSourceIds] = useState<ReadonlySet<string>>(() => new Set());
  const anySyncing = registrySyncingIds(syncingIds, sourceIds).length > 0;
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  // Every read — mount, a run ending, a poll, a Refresh press — is one bump of
  // this token, and the read effect below is keyed on it. A newer bump cancels
  // the read still in flight, so an older read that answers last cannot put an
  // older history back on screen.
  const [reloadToken, setReloadToken] = useState(0);

  const reload = useCallback((reason: string) => {
    console.debug('[sync-audit] reload requested reason=%s', reason);
    setReloadToken(token => token + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;
    // The history and the labels are read side by side, not together: a slow
    // status list must not hold back the rows or the Refresh button.
    void (async () => {
      console.debug('[sync-audit] load: entry token=%d', reloadToken);
      try {
        const data = await memorySyncAuditLog();
        if (cancelled) {
          console.debug('[sync-audit] load: dropped superseded token=%d', reloadToken);
          return;
        }
        setEntries(data);
        console.debug('[sync-audit] load: ok token=%d entries=%d', reloadToken, data.length);
      } catch (err) {
        console.error('[sync-audit] fetch failed', err);
      } finally {
        if (!cancelled) {
          setLoading(false);
          setRefreshing(false);
        }
      }
    })();
    // Labels and source ids only. A failed or superseded read keeps what the
    // last read that answered said, and a row with no label names itself by
    // its scope.
    void (async () => {
      try {
        const statuses = await memorySourcesStatusList();
        if (cancelled) return;
        setLabels(sourceLabelsById(statuses));
        setSourceIds(new Set(statuses.map(status => status.source_id)));
      } catch (err) {
        if (!cancelled) {
          console.warn('[sync-audit] status list failed; keeping the last labels', err);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [reloadToken]);

  // A run ending is when the history changes, whichever tab started the run.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const unsubscribe = subscribeTerminalSyncEvents(({ rowId, stage }) => {
      console.debug('[sync-audit] run ended rowId=%s stage=%s', rowId, stage);
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = undefined;
        reload('run-ended');
      }, REFETCH_AFTER_RUN_ENDS_MS);
    });
    return () => {
      unsubscribe();
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [reload]);

  // While anything is syncing, keep reading: a run whose end the socket never
  // delivered still reaches the history on the next tick.
  useEffect(() => {
    if (!anySyncing) return undefined;
    const id = setInterval(() => {
      reload('poll');
    }, POLL_WHILE_SYNCING_MS);
    return () => clearInterval(id);
  }, [anySyncing, reload]);

  const refreshButton = (
    <Button
      variant="secondary"
      size="xs"
      analyticsId="sync-history-refresh"
      data-testid="sync-history-refresh"
      disabled={refreshing}
      onClick={() => {
        setRefreshing(true);
        reload('manual');
      }}>
      {t('common.refresh', 'Refresh')}
    </Button>
  );

  if (loading) {
    return (
      <div className="text-xs text-content-faint py-2">{t('common.loading', 'Loading...')}</div>
    );
  }

  if (entries.length === 0) {
    return (
      <div className="flex items-center justify-between gap-3 py-2">
        <span className="text-xs text-content-faint">
          {t('sync.noAuditEntries', 'No sync runs recorded yet.')}
        </span>
        {refreshButton}
      </div>
    );
  }

  const totalCost = entries.reduce((s, e) => s + e.estimated_cost_usd, 0);
  const totalInput = entries.reduce((s, e) => s + e.input_tokens, 0);
  const totalOutput = entries.reduce((s, e) => s + e.output_tokens, 0);

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-4 text-xs text-content-muted">
        <span>
          {entries.length} {t('sync.runs', 'sync runs')}
        </span>
        <span className="text-content-faint">·</span>
        <span>
          {formatTokens(totalInput)} in / {formatTokens(totalOutput)} out
        </span>
        <span className="text-content-faint">·</span>
        <span className="font-medium">
          ${totalCost.toFixed(4)} {t('sync.totalCost', 'total')}
        </span>
        <span className="ml-auto">{refreshButton}</span>
      </div>
      <div className="max-h-48 overflow-y-auto rounded-md border border-line-subtle">
        <Table className="text-xs">
          <TableHeader className="sticky top-0 bg-surface-muted text-content-muted">
            <TableRow className="hover:bg-transparent">
              <TableHead className="h-auto px-3 py-1.5 text-left text-xs font-medium">
                {t('sync.when', 'When')}
              </TableHead>
              <TableHead className="h-auto px-3 py-1.5 text-left text-xs font-medium">
                {t('sync.source', 'Source')}
              </TableHead>
              <TableHead className="h-auto px-3 py-1.5 text-right text-xs font-medium">
                {t('sync.items', 'Items')}
              </TableHead>
              <TableHead className="h-auto px-3 py-1.5 text-right text-xs font-medium">
                {t('sync.tokens', 'Tokens')}
              </TableHead>
              <TableHead className="h-auto px-3 py-1.5 text-right text-xs font-medium">
                {t('sync.cost', 'Cost')}
              </TableHead>
              <TableHead className="h-auto px-3 py-1.5 text-right text-xs font-medium">
                {t('sync.duration', 'Duration')}
              </TableHead>
              <TableHead className="h-auto px-3 py-1.5 text-center text-xs font-medium" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {entries.map((e, i) => (
              <TableRow key={`${e.timestamp}-${i}`} className="border-line-subtle">
                <TableCell
                  className="px-3 py-1.5 text-content-secondary whitespace-nowrap"
                  title={e.timestamp}>
                  {timeAgo(e.timestamp, t)}
                </TableCell>
                <TableCell
                  className="px-3 py-1.5 text-content-secondary truncate max-w-[180px]"
                  title={e.scope}>
                  {labels[e.source_id] ?? scopeLabel(e.scope)}
                </TableCell>
                <TableCell className="px-3 py-1.5 text-right tabular-nums text-content-secondary">
                  {e.items_fetched}
                </TableCell>
                <TableCell
                  className="px-3 py-1.5 text-right tabular-nums text-content-secondary"
                  title={`${e.input_tokens} in / ${e.output_tokens} out`}>
                  {formatTokens(e.input_tokens + e.output_tokens)}
                </TableCell>
                <TableCell className="px-3 py-1.5 text-right tabular-nums font-medium text-content-secondary">
                  ${e.estimated_cost_usd.toFixed(4)}
                </TableCell>
                <TableCell className="px-3 py-1.5 text-right tabular-nums text-content-muted">
                  {formatDuration(e.duration_ms)}
                </TableCell>
                <TableCell className="px-3 py-1.5 text-center">
                  {e.success ? (
                    <span className="text-sage-500" title={t('sync.status.success', 'Success')}>
                      ✓
                    </span>
                  ) : (e.tree_ingest_failures ?? 0) > 0 || e.tree_error ? (
                    // openhuman#5820: the fetch committed but the memory-tree
                    // half dropped items — a distinct partial verdict, not the
                    // plain ✗ (which reads as "nothing was fetched"). The
                    // tooltip carries the core's tree_error so the row
                    // explains itself.
                    <span
                      className="text-amber-500"
                      title={
                        e.tree_error ?? t('sync.status.partial', 'Fetched, memory ingest failed')
                      }>
                      ⚠
                    </span>
                  ) : (
                    <span
                      className="text-coral-500"
                      title={e.error ?? t('sync.status.failed', 'Failed')}>
                      ✗
                    </span>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
