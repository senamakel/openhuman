import { useDispatch } from 'react-redux';
import { useNavigate } from 'react-router-dom';

import { useT } from '../../lib/i18n/I18nContext';
import { setSelectedThread } from '../../store/threadSlice';
import type { SubconsciousMode } from '../../utils/tauriCommands/heartbeat';
import type { SubconsciousStatus } from '../../utils/tauriCommands/subconscious';
import SubconsciousReflectionCards from './SubconsciousReflectionCards';

interface ModeOption {
  id: SubconsciousMode;
  titleKey: string;
  descKey: string;
}

const MODE_OPTIONS: ModeOption[] = [
  { id: 'off', titleKey: 'subconscious.mode.off.title', descKey: 'subconscious.mode.off.desc' },
  {
    id: 'simple',
    titleKey: 'subconscious.mode.simple.title',
    descKey: 'subconscious.mode.simple.desc',
  },
  {
    id: 'aggressive',
    titleKey: 'subconscious.mode.aggressive.title',
    descKey: 'subconscious.mode.aggressive.desc',
  },
];

interface IntelligenceSubconsciousTabProps {
  status: SubconsciousStatus | null;
  mode: SubconsciousMode;
  triggerTick: () => Promise<void>;
  triggering: boolean;
  settingMode: boolean;
  setMode: (mode: SubconsciousMode) => Promise<void>;
}

export default function IntelligenceSubconsciousTab({
  status,
  mode,
  triggerTick,
  triggering,
  settingMode,
  setMode,
}: IntelligenceSubconsciousTabProps) {
  const { t } = useT();
  const navigate = useNavigate();
  const dispatch = useDispatch();
  const providerUnavailable = status?.provider_available === false;
  const providerUnavailableReason = providerUnavailable
    ? (status?.provider_unavailable_reason ?? t('subconscious.providerUnavailableTitle'))
    : null;
  const isEnabled = mode !== 'off';

  const handleNavigateToThread = (threadId: string) => {
    dispatch(setSelectedThread(threadId));
    navigate('/chat');
  };

  const handleRunTick = async () => {
    try {
      await triggerTick();
    } catch (error) {
      console.debug('[subconscious-ui] run tick:error', {
        error: error instanceof Error ? error.message : String(error),
      });
    }
  };

  return (
    <div className="space-y-6 animate-fade-up">
      {/* Mode selector */}
      <div>
        <h3 className="text-sm font-semibold text-stone-900 dark:text-neutral-100 mb-2">
          {t('subconscious.mode.label')}
        </h3>
        <div className="grid gap-2">
          {MODE_OPTIONS.map(opt => (
            <button
              key={opt.id}
              type="button"
              disabled={settingMode}
              onClick={() => void setMode(opt.id)}
              className={`text-left rounded-lg border p-3 transition ${
                mode === opt.id
                  ? 'border-primary-500 bg-primary-50 dark:bg-primary-500/10'
                  : 'border-stone-200 dark:border-neutral-800 hover:border-primary-300 dark:hover:border-primary-500/40'
              } ${settingMode ? 'opacity-60 cursor-wait' : ''}`}>
              <div className="flex items-center gap-2">
                <span
                  className={`inline-block w-3 h-3 rounded-full border-2 ${
                    mode === opt.id
                      ? 'bg-primary-500 border-primary-500'
                      : 'border-stone-300 dark:border-neutral-600'
                  }`}
                />
                <span className="text-sm font-medium text-stone-900 dark:text-neutral-100">
                  {t(opt.titleKey)}
                </span>
              </div>
              <p className="mt-1 ml-5 text-xs text-stone-500 dark:text-neutral-400">
                {t(opt.descKey)}
              </p>
            </button>
          ))}
        </div>
        {mode === 'aggressive' && (
          <p className="mt-2 text-xs text-amber-600 dark:text-amber-400">
            {t('subconscious.mode.aggressiveWarning')}
          </p>
        )}
      </div>

      {/* Status bar + Run Now */}
      {isEnabled && (
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
          <button
            onClick={() => void handleRunTick()}
            disabled={triggering || providerUnavailable}
            title={providerUnavailable ? t('subconscious.providerUnavailableTitle') : undefined}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs bg-stone-50 dark:bg-neutral-800/60 hover:bg-stone-100 dark:hover:bg-neutral-800 disabled:opacity-40 border border-stone-200 dark:border-neutral-800 rounded-lg text-stone-600 dark:text-neutral-300 transition-colors">
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
      )}

      {isEnabled && providerUnavailable && (
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

      {isEnabled && (
        <SubconsciousReflectionCards
          onNavigateToThread={handleNavigateToThread}
          pollIntervalMs={15_000}
        />
      )}
    </div>
  );
}
