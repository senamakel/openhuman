/**
 * Unified memory sources panel.
 *
 * Single source of truth for **what feeds memory**: folders, GitHub
 * repos, RSS feeds, web pages, Twitter queries, and Composio
 * integrations. Polls `openhuman.memory_sources_status_list` every 5s
 * for per-source chunk counts and freshness. The Sync button on each
 * row dispatches `openhuman.memory_sources_sync` which runs in the
 * background and emits MemorySyncStageChanged events.
 */
import { useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { useT } from '../../lib/i18n/I18nContext';
import { CoreStateContext } from '../../providers/coreStateContext';
import {
  applyAllIn,
  listMemorySources,
  type MemorySourceEntry,
  memorySourcesStatusList,
  removeMemorySource,
  type SourceStatus,
  syncMemorySource,
  updateMemorySource,
} from '../../services/memorySourcesService';
import type {
  ConfirmationModal as ConfirmationModalType,
  ToastNotification,
} from '../../types/intelligence';
import {
  memoryTreeFlushSource,
  memoryTreePipelineStatus,
  type MemoryTreePipelineStatus,
} from '../../utils/tauriCommands/memoryTree';
import Button from '../ui/Button';
import { AddMemorySourceDialog } from './AddMemorySourceDialog';
import { ConfirmationModal } from './ConfirmationModal';
import { MemorySourceRow } from './MemorySourceRow';
import { AllInIcon, PlusIcon } from './memorySourcesIcons';
import { sourceTreeScope } from './memorySourcesRowHelpers';
import {
  STAGE_FALLBACK_PERCENT,
  type SyncProgress,
  type SyncResult,
} from './memorySourcesSyncTypes';
import { MemorySyncSchedule } from './MemorySyncSchedule';

interface MemorySourcesRegistryProps {
  onToast?: (toast: Omit<ToastNotification, 'id'>) => void;
  pollIntervalMs?: number;
}

/**
 * Parse a sync progress detail string into a 0–100 percent.
 *
 * - Recognises "N/M ..." numeric patterns and returns N/M as a ratio.
 * - Falls back to the per-stage baseline when no ratio is present rather
 *   than returning a bogus number (RC#4, issue #3295).
 * - Returns `null` when both approaches are unavailable (no stage either).
 */
export function parseSyncProgress(detail: string | null, stage?: string): number | null {
  // Try the numeric "N/M ..." ratio first.
  if (detail) {
    const match = detail.match(/^(\d+)\/(\d+)[\s/]/);
    if (match) {
      const current = parseInt(match[1], 10);
      const total = parseInt(match[2], 10);
      if (total > 0) return Math.round((current / total) * 100);
    }
  }
  // Fall back to the per-stage baseline percentage.
  if (stage && stage in STAGE_FALLBACK_PERCENT) {
    return STAGE_FALLBACK_PERCENT[stage];
  }
  return null;
}

/**
 * Parse the number of newly-ingested items from a `completed` stage detail
 * string. The backend formats this as `"ingested N item(s)"`
 * (`memory_sources/sync.rs`). Returns `null` when no count is present so the
 * UI can fall back to a generic "synced" confirmation (#3295).
 */
export function parseIngestedCount(detail: string | null): number | null {
  if (!detail) return null;
  const match = detail.match(/ingested\s+(\d+)\s+item/i);
  if (match) return parseInt(match[1], 10);
  return null;
}

export function MemorySourcesRegistry({
  onToast,
  pollIntervalMs = 5000,
}: MemorySourcesRegistryProps) {
  const { t } = useT();
  const navigate = useNavigate();
  // Read the core snapshot directly (not via the throwing `useCoreState`
  // hook) so this component still renders in unit tests that mount it
  // without a CoreStateProvider — there `ctx` is null and `isAuthenticated`
  // stays a stable `false`, so the load effect behaves exactly as before.
  const coreState = useContext(CoreStateContext);
  const isAuthenticated = coreState?.snapshot.auth.isAuthenticated ?? false;
  const [sources, setSources] = useState<MemorySourceEntry[]>([]);
  const [statuses, setStatuses] = useState<SourceStatus[]>([]);
  // Global downstream pipeline health (GH-4690) — the same snapshot the
  // Brain > Memory > Sync panel renders. Folded into each row so a source that
  // ingested but failed embeddings/extraction/tree-build shows a warning rather
  // than a clean synced badge. `null` until the first poll resolves.
  const [pipeline, setPipeline] = useState<MemoryTreePipelineStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [dialogOpen, setDialogOpen] = useState(false);
  // RC#1 (#3295): use a Set so multiple sources can show "syncing" concurrently.
  // Set state is always replaced with a new Set to trigger re-renders.
  const [syncingIds, setSyncingIds] = useState<Set<string>>(new Set());
  const [buildingId, setBuildingId] = useState<string | null>(null);
  const [syncProgress, setSyncProgress] = useState<Map<string, SyncProgress>>(new Map());
  // Terminal per-source result (success/failure) shown after a sync ends (#3295).
  const [syncResults, setSyncResults] = useState<Map<string, SyncResult>>(new Map());
  const [allInModalOpen, setAllInModalOpen] = useState(false);
  const [applyingAllIn, setApplyingAllIn] = useState(false);
  const allInInFlightRef = useRef(false);
  const [expandedSettingsId, setExpandedSettingsId] = useState<string | null>(null);

  // Refs let the (intentionally dep-free) sync-stage listener fire accurate
  // toasts on the *terminal* event without re-subscribing on every render or
  // 5s poll. The handler must read the latest onToast/sources/t (#3295).
  const onToastRef = useRef(onToast);
  const sourcesRef = useRef(sources);
  const tRef = useRef(t);
  useEffect(() => {
    onToastRef.current = onToast;
    sourcesRef.current = sources;
    tRef.current = t;
  });

  useEffect(() => {
    const handler = (e: Event) => {
      const data = (e as CustomEvent).detail as {
        stage?: string;
        /** Originating memory-source id (RC#2, #3295). Preferred over connection_id. */
        source_id?: string | null;
        /** Legacy: document/connection id. Still present for backward compat. */
        connection_id?: string | null;
        detail?: string;
      } | null;

      // RC#2 (#3295): prefer source_id when present; fall back to connection_id for
      // backward compat with older core versions that don't emit source_id yet.
      const rowId = data?.source_id ?? data?.connection_id;
      if (!rowId) return;

      const stage = data?.stage ?? '';

      console.debug(
        `[ui-flow][memory-sync] stage=${stage} rowId=${rowId} source_id=${data?.source_id ?? 'absent'} connection_id=${data?.connection_id ?? 'absent'}`
      );

      if (stage === 'completed' || stage === 'failed') {
        // Clear the live progress bar + syncing flag for this row.
        setSyncProgress(prev => {
          const next = new Map(prev);
          next.delete(rowId);
          return next;
        });
        // RC#1: immutable Set update — remove just this source, keep others syncing.
        setSyncingIds(prev => {
          const next = new Set(prev);
          next.delete(rowId);
          return next;
        });

        const tt = tRef.current;
        const label = sourcesRef.current.find(s => s.id === rowId)?.label ?? rowId;

        if (stage === 'completed') {
          // Success: record + toast the item count parsed from the detail
          // ("ingested N item(s)"). 0 new items → "up to date" (#3295).
          const items = parseIngestedCount(data?.detail ?? null);
          setSyncResults(prev => {
            const next = new Map(prev);
            next.set(rowId, { kind: 'success', items, reason: null });
            return next;
          });
          onToastRef.current?.({
            type: 'success',
            title: `${tt('memorySources.sync.completeTitle')} ${label}`,
            message:
              items && items > 0
                ? `${items} ${tt('memorySources.sync.itemsSynced')}`
                : tt('memorySources.sync.upToDate'),
          });
        } else {
          // Failure: surface the reason on the row + a toast. The core already
          // reported internal bugs to Sentry via report_error_or_expected.
          const reason = data?.detail ?? null;
          setSyncResults(prev => {
            const next = new Map(prev);
            next.set(rowId, { kind: 'failed', items: null, reason });
            return next;
          });
          onToastRef.current?.({
            type: 'error',
            title: `${tt('memorySources.sync.failedLabel')} · ${label}`,
            message: reason ?? tt('memorySources.sync.failedLabel'),
          });
        }
        return;
      }

      // Non-terminal stage: a sync is genuinely in progress. Drop any stale
      // terminal result for this row so the live bar replaces the old chip.
      setSyncResults(prev => {
        if (!prev.has(rowId)) return prev;
        const next = new Map(prev);
        next.delete(rowId);
        return next;
      });
      const percent = parseSyncProgress(data?.detail ?? null, stage);
      setSyncProgress(prev => {
        const next = new Map(prev);
        next.set(rowId, { stage, detail: data?.detail ?? null, percent });
        return next;
      });
      // RC#1: ADD this source id to the set (immutable update).
      if (
        stage === 'requested' ||
        stage === 'fetching' ||
        stage === 'stored' ||
        stage === 'queued' ||
        stage === 'ingesting'
      ) {
        setSyncingIds(prev => {
          if (prev.has(rowId)) return prev; // no change — avoid re-render
          const next = new Set(prev);
          next.add(rowId);
          return next;
        });
      }
    };
    window.addEventListener('openhuman:memory-sync-stage', handler);
    return () => window.removeEventListener('openhuman:memory-sync-stage', handler);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [list, stats, health] = await Promise.all([
        listMemorySources().catch(err => {
          console.warn('[ui-flow][memory-sources] list failed', err);
          return [] as MemorySourceEntry[];
        }),
        memorySourcesStatusList().catch(err => {
          console.warn('[ui-flow][memory-sources] status_list failed', err);
          return [] as SourceStatus[];
        }),
        // GH-4690: downstream pipeline health. Best-effort — a failure here
        // must never hide the source list, so we fall back to `null` (rows then
        // rely on the precise per-source pending-chunk signal alone).
        memoryTreePipelineStatus().catch(err => {
          console.warn('[ui-flow][memory-sources] pipeline_status failed', err);
          return null;
        }),
      ]);
      setSources(list);
      setStatuses(stats);
      setPipeline(health);
      // RC#5 (#3295): The 5s poll is the safety net for missed completed/failed events.
      // If a source is in syncingIds but the poll shows it's no longer active (no
      // in-progress status indicator from the server), we clear it here. In practice
      // the event stream covers this; on remount the state rehydrates within ~5s via poll.
      // No new RPC needed — reconciliation is best-effort and relies on the existing poll.
    } finally {
      setLoading(false);
    }
  }, []);

  // Load on mount AND whenever the session transitions to authenticated.
  // After a page reload the registry can mount (e.g. via a persisted
  // `?tab=memory` deep link) *before* CoreStateProvider has restored the
  // session, so the initial fetch runs against a not-yet-ready core and
  // surfaces nothing. Re-running when `isAuthenticated` flips true picks up
  // sources immediately instead of waiting for the next 5s poll — which
  // under CI load was racing the E2E visibility timeout (#3449).
  useEffect(() => {
    void refresh();
  }, [refresh, isAuthenticated]);

  useEffect(() => {
    if (!pollIntervalMs) return undefined;
    const id = setInterval(() => {
      void refresh();
    }, pollIntervalMs);
    return () => clearInterval(id);
  }, [pollIntervalMs, refresh]);

  const statusById = useMemo(() => {
    const m = new Map<string, SourceStatus>();
    for (const s of statuses) m.set(s.source_id, s);
    return m;
  }, [statuses]);

  // Newest chunk timestamp across every source — the "Last synced …" anchor
  // for the global schedule header. Derived from persisted chunk data, so it
  // survives restarts.
  const overallLastSyncMs = useMemo(() => {
    let newest: number | null = null;
    for (const s of statuses) {
      if (s.last_chunk_at_ms != null && (newest === null || s.last_chunk_at_ms > newest)) {
        newest = s.last_chunk_at_ms;
      }
    }
    return newest;
  }, [statuses]);

  const handleToggle = useCallback(
    async (source: MemorySourceEntry) => {
      try {
        const updated = await updateMemorySource(source.id, { enabled: !source.enabled });
        setSources(prev => prev.map(s => (s.id === updated.id ? updated : s)));
      } catch (err) {
        onToast?.({
          type: 'error',
          title: t('memorySources.toggleFailed'),
          message: err instanceof Error ? err.message : String(err),
        });
      }
    },
    [onToast, t]
  );

  const handleRemove = useCallback(
    async (source: MemorySourceEntry) => {
      try {
        await removeMemorySource(source.id);
        setSources(prev => prev.filter(s => s.id !== source.id));
        onToast?.({ type: 'success', title: t('memorySources.removed'), message: source.label });
      } catch (err) {
        onToast?.({
          type: 'error',
          title: t('memorySources.removeFailed'),
          message: err instanceof Error ? err.message : String(err),
        });
      }
    },
    [onToast, t]
  );

  const handleSync = useCallback(
    async (source: MemorySourceEntry) => {
      // RC#1 (#3295): add immediately on click — event will also fire, but this
      // ensures the row lights up before the first sync-stage event arrives.
      setSyncingIds(prev => {
        const next = new Set(prev);
        next.add(source.id);
        return next;
      });
      // A fresh sync is starting — drop any prior terminal result chip (#3295).
      setSyncResults(prev => {
        if (!prev.has(source.id)) return prev;
        const next = new Map(prev);
        next.delete(source.id);
        return next;
      });
      console.debug(`[ui-flow][memory-sync] manual sync triggered source_id=${source.id}`);
      try {
        await syncMemorySource(source.id);
        // NOTE: success/failure feedback is intentionally NOT fired here. This
        // RPC returns in ~4ms after merely *spawning* the background sync; the
        // real outcome arrives via the terminal `completed`/`failed` stage event
        // (handled above), which carries the item count / failure reason (#3295).
        void refresh();
      } catch (err) {
        // The RPC call itself failed (transport/validation) — the background
        // sync never started, so no stage event will arrive. Surface it here:
        // clear the syncing flag and record a failed result on the row.
        const reason = err instanceof Error ? err.message : String(err);
        setSyncingIds(prev => {
          const next = new Set(prev);
          next.delete(source.id);
          return next;
        });
        setSyncResults(prev => {
          const next = new Map(prev);
          next.set(source.id, { kind: 'failed', items: null, reason });
          return next;
        });
        onToast?.({
          type: 'error',
          title: `${t('memorySources.sync.failedLabel')} · ${source.label}`,
          message: reason,
        });
      }
      // No `finally` clear: on success the row stays "syncing" until the
      // terminal stage event arrives (the sync is still running in the
      // background). Clearing here is what made the indicator vanish in ~4ms.
    },
    [onToast, refresh, t]
  );

  const handleBuild = useCallback(
    async (source: MemorySourceEntry) => {
      const scope = sourceTreeScope(source);
      if (!scope) return;
      setBuildingId(source.id);
      try {
        const resp = await memoryTreeFlushSource(scope);
        onToast?.({
          type: 'success',
          title: t('memorySources.build.successTitle'),
          message: `${resp.seals_fired} ${t('memorySources.build.sealsMessage')}`,
        });
      } catch (err) {
        onToast?.({
          type: 'error',
          title: t('memorySources.build.failedTitle'),
          message: err instanceof Error ? err.message : String(err),
        });
      } finally {
        setBuildingId(prev => (prev === source.id ? null : prev));
      }
    },
    [onToast, t]
  );

  const handleAdded = useCallback(
    (source: MemorySourceEntry) => {
      setSources(prev => [...prev, source]);
      onToast?.({ type: 'success', title: t('memorySources.added'), message: source.label });
      void refresh();
    },
    [onToast, refresh, t]
  );

  const handleConfirmAllIn = useCallback(async () => {
    if (allInInFlightRef.current) return;
    allInInFlightRef.current = true;
    setApplyingAllIn(true);
    try {
      const result = await applyAllIn();
      setSources(result.sources);
      // openhuman#5820: the RPC resolves even when triggers failed, so the
      // verdict comes from the counts, not from "the call did not throw".
      // Every trigger failing (the incident: `no memory source registered`
      // x4) is an error; a partial start is a warning that names both
      // counts. Only a clean sweep is a success.
      if (result.sync_failed > 0 && result.sync_triggered === 0) {
        onToast?.({
          type: 'error',
          title: t('memorySources.allIn.allFailed'),
          message: result.sync_errors[0],
        });
      } else if (result.sync_failed > 0) {
        onToast?.({
          type: 'warning',
          title: t('memorySources.allIn.partial')
            .replace('{triggered}', String(result.sync_triggered))
            .replace('{failed}', String(result.sync_failed)),
          message: result.sync_errors[0],
        });
      } else {
        onToast?.({ type: 'success', title: t('memorySources.allIn.success') });
      }
    } catch (err) {
      onToast?.({
        type: 'error',
        title: t('memorySources.allIn.failed'),
        message: err instanceof Error ? err.message : String(err),
      });
    } finally {
      allInInFlightRef.current = false;
      setApplyingAllIn(false);
      setAllInModalOpen(false);
    }
  }, [onToast, t]);

  const handleSettingsSaved = useCallback((updated: MemorySourceEntry) => {
    setSources(prev => prev.map(s => (s.id === updated.id ? updated : s)));
  }, []);

  const handleToggleSettings = useCallback((sourceId: string) => {
    setExpandedSettingsId(prev => (prev === sourceId ? null : sourceId));
  }, []);

  const allInModal: ConfirmationModalType = {
    isOpen: allInModalOpen,
    title: t('memorySources.allIn.title'),
    message: t('memorySources.allIn.message'),
    confirmText: t('memorySources.allIn.confirm'),
    cancelText: t('memorySources.allIn.cancel'),
    destructive: false,
    onConfirm: () => {
      void handleConfirmAllIn();
    },
    onCancel: () => {
      setAllInModalOpen(false);
    },
  };

  return (
    <section className="rounded-lg border border-line bg-surface p-4" data-testid="memory-sources">
      <header className="mb-3 flex items-center justify-between gap-2">
        <h3 className="text-sm font-semibold text-content-secondary">{t('memorySources.title')}</h3>
        <div className="flex items-center gap-2">
          <Button
            variant="secondary"
            size="sm"
            onClick={() => setAllInModalOpen(true)}
            disabled={applyingAllIn}
            data-testid="all-in-button"
            leadingIcon={<AllInIcon />}
            className="border-primary-300 text-primary-600 hover:bg-primary-50
                       dark:border-primary-500/30 dark:text-primary-400 dark:hover:bg-primary-500/10">
            {t('memorySources.allIn.button')}
          </Button>
          <Button
            variant="primary"
            size="sm"
            onClick={() => setDialogOpen(true)}
            leadingIcon={<PlusIcon />}>
            {t('memorySources.addSource')}
          </Button>
        </div>
      </header>

      <MemorySyncSchedule lastSyncMs={overallLastSyncMs} onToast={onToast} />

      {loading ? (
        <p className="text-xs text-content-muted">{t('common.loading')}</p>
      ) : sources.length === 0 ? (
        <p className="text-xs text-content-muted">{t('memorySources.empty')}</p>
      ) : (
        <ul className="divide-y divide-line-subtle">
          {sources.map(source => (
            <MemorySourceRow
              key={source.id}
              source={source}
              status={statusById.get(source.id) ?? null}
              pipeline={pipeline}
              isAuthenticated={isAuthenticated}
              isSyncing={syncingIds.has(source.id) || syncProgress.has(source.id)}
              isBuilding={buildingId === source.id}
              progress={syncProgress.get(source.id) ?? null}
              result={syncResults.get(source.id) ?? null}
              settingsExpanded={expandedSettingsId === source.id}
              onToggle={handleToggle}
              onRemove={handleRemove}
              onSync={handleSync}
              onBuild={handleBuild}
              onToggleSettings={handleToggleSettings}
              onSettingsSaved={handleSettingsSaved}
              onToast={onToast}
              onViewHealth={() => {
                console.debug('[ui-flow][memory-sources] view memory health from source row');
                navigate('/brain?tab=sync');
              }}
              onSignIn={() => {
                console.debug(
                  '[ui-flow][memory-sources] sign-in prompt from stored-without-vectors'
                );
                navigate('/auth');
              }}
            />
          ))}
        </ul>
      )}

      <AddMemorySourceDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onAdded={handleAdded}
      />

      {allInModalOpen && (
        <ConfirmationModal modal={allInModal} onClose={() => setAllInModalOpen(false)} />
      )}
    </section>
  );
}
