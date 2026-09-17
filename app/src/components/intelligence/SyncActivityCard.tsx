/**
 * "Now syncing" on Brain › Sync — the sources with a sync in flight, and where
 * each one is (openhuman#6257).
 *
 * The tab showed the history of finished runs and the job counters, but
 * nothing about a run still going, so a source whose Sources row said "Queued"
 * had no counterpart here. This lists them from the same store the Sources
 * rows read, so the two tabs agree.
 *
 * The status list is polled while the card is mounted for three things: each
 * source's label (the store keys runs by id); the registry's source ids, since
 * the store also lights rows keyed by a document outside the registry, which
 * never end and are not listed here; and the core's own account of what is in
 * flight, which seeds a run this page never saw start (an app reload mid-sync).
 */
import { useEffect, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { memorySourcesStatusList } from '../../services/memorySourcesService';
import {
  registrySyncingIds,
  showsStageDetail,
  sourceLabelsById,
  stageLabelKey,
} from './memorySourcesSyncTypes';
import { reconcileWithStatuses, useMemorySyncActivity } from './memorySyncActivityStore';

/** How often the card re-reads the status list while mounted. */
export const STATUS_POLL_MS = 10_000;

export function SyncActivityCard() {
  const { t } = useT();
  const { syncingIds, progress } = useMemorySyncActivity();
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [sourceIds, setSourceIds] = useState<ReadonlySet<string>>(() => new Set());

  useEffect(() => {
    let cancelled = false;
    // Polls can answer out of order: a read slower than the poll interval must
    // not put an older account back over a newer one.
    let requested = 0;
    let applied = 0;
    const refresh = async () => {
      const seq = ++requested;
      try {
        const statuses = await memorySourcesStatusList();
        if (cancelled) return;
        if (seq < applied) {
          console.debug(
            '[sync-activity] dropped an older status read seq=%d applied=%d',
            seq,
            applied
          );
          return;
        }
        applied = seq;
        setLabels(sourceLabelsById(statuses));
        setSourceIds(new Set(statuses.map(status => status.source_id)));
        reconcileWithStatuses(statuses);
        console.debug('[sync-activity] status list ok sources=%d', statuses.length);
      } catch (err) {
        // A failed read keeps the labels and source ids of the last read that
        // answered, so runs the socket reports for those sources still show.
        if (!cancelled) console.warn('[sync-activity] status list failed', err);
      }
    };
    void refresh();
    const id = setInterval(() => {
      void refresh();
    }, STATUS_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  const rows = registrySyncingIds(syncingIds, sourceIds);

  return (
    <div className="space-y-2" data-testid="sync-activity-card">
      <h3 className="text-sm font-medium text-content-secondary">
        {t('sync.nowSyncing.title', 'Now syncing')}
      </h3>
      {rows.length === 0 ? (
        <div className="text-xs text-content-faint" data-testid="sync-activity-empty">
          {t('sync.nowSyncing.empty', 'Nothing is syncing right now.')}
        </div>
      ) : (
        <ul className="space-y-1.5">
          {rows.map(id => {
            const live = progress.get(id);
            // A row the Sync button lit before the core's first stage has no
            // bar yet: it is starting.
            const stage = live?.stage ?? 'requested';
            const detail = live && showsStageDetail(live.stage) ? live.detail : null;
            return (
              <li
                key={id}
                data-testid={`sync-activity-row-${id}`}
                className="flex items-center justify-between gap-3 rounded-md border border-line-subtle px-3 py-2 text-xs">
                <span className="truncate font-medium text-content">{labels[id] ?? id}</span>
                <span className="flex min-w-0 items-center gap-2 text-content-muted">
                  <span className="shrink-0">{t(stageLabelKey(stage))}</span>
                  {detail ? <span className="truncate text-content-faint">{detail}</span> : null}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
