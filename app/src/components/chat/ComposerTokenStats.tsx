import { useT } from '../../lib/i18n/I18nContext';
import { useAppSelector } from '../../store/hooks';

function fmt(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}K`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

export default function ComposerTokenStats() {
  const { t } = useT();
  const usage = useAppSelector(state => state.chatRuntime.sessionTokenUsage);

  const totalTokens = usage.inputTokens + usage.outputTokens;
  if (totalTokens === 0) return null;

  return (
    <div className="flex items-center gap-3 mt-1.5 text-[10px] font-mono text-stone-400 dark:text-neutral-500 select-none">
      <span title={t('token.inputTokens')}>
        {t('token.inLabel')} {fmt(usage.inputTokens)}
      </span>
      <span className="text-stone-300 dark:text-neutral-700">·</span>
      <span title={t('token.outputTokens')}>
        {t('token.outLabel')} {fmt(usage.outputTokens)}
      </span>
      <span className="text-stone-300 dark:text-neutral-700">·</span>
      <span title={t('token.turnsCount')}>
        {usage.turns} {usage.turns === 1 ? t('token.turn') : t('token.turns')}
      </span>
    </div>
  );
}
