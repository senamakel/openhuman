import { useDispatch } from 'react-redux';
import { useNavigate } from 'react-router-dom';

import { useT } from '../../lib/i18n/I18nContext';
import { setSelectedThread } from '../../store/threadSlice';
import type { SubconsciousStatus } from '../../utils/tauriCommands/subconscious';
import SubconsciousReflectionCards from './SubconsciousReflectionCards';

function formatInterval(minutes: number, t: (key: string) => string): string {
  switch (minutes) {
    case 5:
      return t('subconscious.interval.fiveMinutes');
    case 10:
      return t('subconscious.interval.tenMinutes');
    case 15:
      return t('subconscious.interval.fifteenMinutes');
    case 30:
      return t('subconscious.interval.thirtyMinutes');
    case 60:
      return t('subconscious.interval.oneHour');
    case 360:
      return t('subconscious.interval.sixHours');
    case 720:
      return t('subconscious.interval.twelveHours');
    case 1440:
      return t('subconscious.interval.oneDay');
    default:
      return String(minutes);
  }
}

interface IntelligenceSubconsciousTabProps {
  status: SubconsciousStatus | null;
  triggerTick: () => Promise<void>;
  triggering: boolean;
}

export default function IntelligenceSubconsciousTab({
  status,
  triggerTick,
  triggering,
}: IntelligenceSubconsciousTabProps) {
  const { t } = useT();
  const navigate = useNavigate();
  const dispatch = useDispatch();
  const providerUnavailable = status?.provider_available === false;
  const providerUnavailableReason = providerUnavailable
    ? (status?.provider_unavailable_reason ?? t('subconscious.providerUnavailableTitle'))
    : null;

  const handleNavigateToThread = (threadId: string) => {
    console.debug('[subconscious-ui] navigate:thread', { threadId });
    dispatch(setSelectedThread(threadId));
    navigate('/chat');
  };

  const handleRunTick = async () => {
    console.debug('[subconscious-ui] run tick:start', { triggering });
    try {
      await triggerTick();
      console.debug('[subconscious-ui] run tick:done');
    } catch (error) {
      console.debug('[subconscious-ui] run tick:error', {
        error: error instanceof Error ? error.message : String(error),
      });
    }
  };

  return (
    <div className="space-y-6 animate-fade-up">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 text-xs text-stone-400 dark:text-neutral-500">
          {status && (
            <>
              <span>
                {status.total_ticks} {t('subconscious.ticks')}
              </span>
              {status.last_tick_at && (
                <>
                  <span className="text-stone-300 dark:text-neutral-600">|</span>
                  <span>
                    {t('subconscious.last')}:{' '}
                    {new Date(status.last_tick_at * 1000).toLocaleTimeString()}
                  </span>
                </>
              )}
              {status.consecutive_failures > 0 && (
                <>
                  <span className="text-stone-300 dark:text-neutral-600">|</span>
                  <span className="text-coral-500">
                    {status.consecutive_failures} {t('subconscious.failed')}
                  </span>
                </>
              )}
            </>
          )}
        </div>
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1.5">
            <svg
              className="w-3 h-3 text-stone-400 dark:text-neutral-500"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
            <select
              value={status?.interval_minutes ?? 5}
              onChange={() => {}}
              disabled
              title={t('subconscious.tickInterval')}
              className="text-xs bg-stone-50 dark:bg-neutral-800/60 border border-stone-200 dark:border-neutral-800 rounded px-1.5 py-0.5 text-stone-500 dark:text-neutral-400 cursor-not-allowed">
              <option value={5}>{formatInterval(5, t)}</option>
              <option value={10}>{formatInterval(10, t)}</option>
              <option value={15}>{formatInterval(15, t)}</option>
              <option value={30}>{formatInterval(30, t)}</option>
              <option value={60}>{formatInterval(60, t)}</option>
            </select>
          </div>
          <button
            onClick={() => void handleRunTick()}
            disabled={triggering || providerUnavailable}
            title={providerUnavailable ? t('subconscious.providerUnavailableTitle') : undefined}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs bg-stone-50 dark:bg-neutral-800/60 hover:bg-stone-100 dark:hover:bg-neutral-800 dark:bg-neutral-800 disabled:opacity-40 border border-stone-200 dark:border-neutral-800 rounded-lg text-stone-600 dark:text-neutral-300 transition-colors">
            {triggering ? (
              <div className="w-3 h-3 border border-stone-400 border-t-transparent rounded-full animate-spin" />
            ) : (
              <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M13 10V3L4 14h7v7l9-11h-7z"
                />
              </svg>
            )}
            {t('subconscious.runNow')}
          </button>
        </div>
      </div>

      {providerUnavailable && (
        <div className="rounded-lg border border-amber-200 dark:border-amber-500/30 bg-amber-50 dark:bg-amber-500/10 p-3">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <p className="text-sm font-medium text-amber-800 dark:text-amber-200">
                {t('subconscious.providerUnavailableTitle')}
              </p>
              <p className="mt-1 text-xs text-amber-700 dark:text-amber-300 break-words">
                {providerUnavailableReason}
              </p>
            </div>
            <button
              type="button"
              onClick={() => navigate('/settings/llm')}
              className="flex-shrink-0 rounded-md bg-amber-600 px-2.5 py-1.5 text-xs font-medium text-white hover:bg-amber-700 transition-colors">
              {t('subconscious.providerSettings')}
            </button>
          </div>
        </div>
      )}

      <SubconsciousReflectionCards
        onNavigateToThread={handleNavigateToThread}
        pollIntervalMs={15_000}
      />
    </div>
  );
}
