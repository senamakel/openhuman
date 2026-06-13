import { useCallback, useEffect, useRef, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { isTauri } from '../../utils/tauriCommands/common';
import {
  subconsciousTriggersStatus,
  type SubconsciousTriggersStatus,
} from '../../utils/tauriCommands/subconscious';

const cardClass =
  'rounded-lg border border-stone-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900';

/**
 * Debug / manage panel for the event-driven subconscious trigger pipeline.
 * Surfaces the `subconscious_triggers.status` RPC: whether the pipeline is
 * enabled, the effective mode, the promotion budget, and live orchestrator
 * runtime state (running flag + pending queue depth). Polls every 5s.
 */
export default function SubconsciousTriggersPanel() {
  const { t } = useT();
  const [status, setStatus] = useState<SubconsciousTriggersStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const inFlight = useRef(false);

  const refresh = useCallback(async () => {
    if (!isTauri() || inFlight.current) return;
    inFlight.current = true;
    try {
      const res = await subconsciousTriggersStatus();
      setStatus(res.result ?? null);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
      inFlight.current = false;
    }
  }, []);

  useEffect(() => {
    // refresh() only setStates asynchronously (after an await); the initial
    // poll + 5s interval mirror the useSubconscious pattern.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh();
    const id = setInterval(() => void refresh(), 5000);
    return () => clearInterval(id);
  }, [refresh]);

  return (
    <div className={cardClass}>
      <div className="mb-3 flex items-center justify-between">
        <div>
          <h3 className="text-sm font-semibold text-stone-900 dark:text-neutral-100">
            {t('subconsciousTriggers.title')}
          </h3>
          <p className="text-xs text-stone-500 dark:text-neutral-400">
            {t('subconsciousTriggers.subtitle')}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void refresh()}
          className="rounded-md border border-stone-200 px-2.5 py-1 text-xs text-stone-600 transition hover:bg-stone-50 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-800">
          {t('common.refresh')}
        </button>
      </div>

      {loading && !status ? (
        <p className="text-xs text-stone-500 dark:text-neutral-400">{t('common.loading')}</p>
      ) : error ? (
        <p className="text-xs text-coral-600 dark:text-coral-400">
          {t('common.error')}: {error}
        </p>
      ) : status ? (
        <div className="space-y-2">
          <StatusRow
            label={t('subconsciousTriggers.pipeline')}
            value={status.triggers_enabled ? t('common.enabled') : t('common.disabled')}
            tone={status.triggers_enabled ? 'good' : 'muted'}
          />
          <StatusRow label={t('subconsciousTriggers.mode')} value={status.mode} />
          <StatusRow
            label={t('subconsciousTriggers.orchestrator')}
            value={
              status.orchestrator_running
                ? t('subconsciousTriggers.running')
                : t('subconsciousTriggers.stopped')
            }
            tone={status.orchestrator_running ? 'good' : 'muted'}
          />
          <StatusRow
            label={t('subconsciousTriggers.promotionsPerHour')}
            value={String(status.max_promotions_per_hour)}
          />
          <StatusRow
            label={t('subconsciousTriggers.queueDepth')}
            value={status.queue_depth === null ? '—' : String(status.queue_depth)}
          />
          <StatusRow
            label={t('subconsciousTriggers.orchestratorThread')}
            value={status.orchestrator_thread_id}
            mono
          />
          <StatusRow
            label={t('subconsciousTriggers.userThread')}
            value={status.user_thread_id}
            mono
          />

          {!status.triggers_enabled && (
            <p className="pt-1 text-xs text-stone-500 dark:text-neutral-400">
              {t('subconsciousTriggers.disabledHint')}
            </p>
          )}
        </div>
      ) : null}
    </div>
  );
}

function StatusRow({
  label,
  value,
  tone = 'default',
  mono = false,
}: {
  label: string;
  value: string;
  tone?: 'default' | 'good' | 'muted';
  mono?: boolean;
}) {
  const toneClass =
    tone === 'good'
      ? 'text-sage-600 dark:text-sage-400'
      : tone === 'muted'
        ? 'text-stone-400 dark:text-neutral-500'
        : 'text-stone-800 dark:text-neutral-200';
  return (
    <div className="flex items-center justify-between gap-3 text-xs">
      <span className="text-stone-500 dark:text-neutral-400">{label}</span>
      <span className={`${toneClass} ${mono ? 'font-mono' : 'font-medium'} truncate`}>{value}</span>
    </div>
  );
}
