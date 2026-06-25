import { useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { useAppSelector } from '../../store/hooks';
import type { SubAgentUsage } from '../../store/chatRuntimeSlice';

/** Fallback context window when the core hasn't reported a real one yet. */
const DEFAULT_CONTEXT_WINDOW = 200_000;

function fmt(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0';
  if (n < 1000) return String(Math.round(n));
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}K`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/** Format a USD cost compactly: sub-cent values keep more precision. */
function fmtUsd(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '$0.00';
  if (n < 0.01) return `$${n.toFixed(4)}`;
  if (n < 1) return `$${n.toFixed(3)}`;
  return `$${n.toFixed(2)}`;
}

function ok(n: number): boolean {
  return Number.isFinite(n) && n > 0;
}

function dot() {
  return <span className="text-stone-300 dark:text-neutral-700">·</span>;
}

interface ComposerTokenStatsProps {
  /** Resolved model id, shown as the leading stat when present. */
  model?: string | null;
}

export default function ComposerTokenStats({ model }: ComposerTokenStatsProps = {}) {
  const { t } = useT();
  const usage = useAppSelector(state => state.chatRuntime.sessionTokenUsage);
  const [open, setOpen] = useState(false);

  const inTok = usage.inputTokens || 0;
  const outTok = usage.outputTokens || 0;
  const cachedTok = usage.cachedTokens || 0;
  const turns = usage.turns || 0;
  const costUsd = usage.costUsd || 0;
  const subAgents: SubAgentUsage[] = Object.values(usage.subAgents ?? {});

  // Still render when only the model is known (no turns yet) so the resolved
  // model stays visible in the composer footer.
  if (turns === 0 && !model) return null;

  const contextWindow = ok(usage.contextWindow) ? usage.contextWindow : DEFAULT_CONTEXT_WINDOW;
  const contextUsed = usage.lastTurnContextUsed || 0;
  const showCtx = ok(contextUsed);
  const contextPct = showCtx ? Math.min(100, Math.round((contextUsed / contextWindow) * 100)) : 0;

  const showIn = ok(inTok);
  const showOut = ok(outTok);
  const showCost = ok(costUsd);

  const parts: React.ReactNode[] = [];

  if (model) {
    parts.push(
      <span key="model" className="truncate" title={model}>
        {model}
      </span>
    );
  }
  if (showIn) {
    parts.push(
      <span key="in" title={t('token.inputTokens')}>
        {t('token.inLabel')} {fmt(inTok)}
      </span>
    );
  }
  if (showOut) {
    parts.push(
      <span key="out" title={t('token.outputTokens')}>
        {t('token.outLabel')} {fmt(outTok)}
      </span>
    );
  }
  if (turns > 0) {
    parts.push(
      <span key="turns" title={t('token.turnsCount')}>
        {turns} {turns === 1 ? t('token.turn') : t('token.turns')}
      </span>
    );
  }
  if (showCtx) {
    parts.push(
      <span key="ctx" title={t('token.contextWindow')}>
        {t('token.ctxLabel')} {contextPct}% ({fmt(contextUsed)}/{fmt(contextWindow)})
      </span>
    );
  }
  if (showCost) {
    parts.push(
      <span key="cost" title={t('token.costTitle')}>
        {fmtUsd(costUsd)}
      </span>
    );
  }

  if (parts.length === 0) return null;

  return (
    <div
      className="relative flex min-w-0 items-center"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)}
      onBlur={() => setOpen(false)}>
      <div
        className="flex min-w-0 flex-wrap items-center gap-2.5 text-[10px] font-mono text-stone-400 dark:text-neutral-500 select-none"
        tabIndex={0}
        role="group"
        aria-label={t('token.sessionUsageTitle')}>
        {parts.map((part, i) => (
          <span key={i} className="contents">
            {i > 0 && dot()}
            {part}
          </span>
        ))}
      </div>
      {open && (
        <div
          data-testid="composer-token-breakdown"
          role="tooltip"
          className="absolute bottom-full left-0 z-50 mb-1.5 w-64 rounded-md border border-stone-200 bg-white p-2.5 text-[11px] shadow-lg dark:border-neutral-700 dark:bg-neutral-800">
          <div className="mb-1.5 font-semibold text-stone-700 dark:text-neutral-200">
            {t('token.sessionUsageTitle')}
          </div>
          <dl className="space-y-0.5 font-mono text-stone-500 dark:text-neutral-400">
            <div className="flex justify-between gap-3">
              <dt>{t('token.inLabel')}</dt>
              <dd className="text-stone-700 dark:text-neutral-200">{fmt(inTok)}</dd>
            </div>
            <div className="flex justify-between gap-3">
              <dt>{t('token.outLabel')}</dt>
              <dd className="text-stone-700 dark:text-neutral-200">{fmt(outTok)}</dd>
            </div>
            {ok(cachedTok) && (
              <div className="flex justify-between gap-3">
                <dt>{t('token.cachedLabel')}</dt>
                <dd className="text-stone-700 dark:text-neutral-200">{fmt(cachedTok)}</dd>
              </div>
            )}
            <div className="flex justify-between gap-3">
              <dt>{t('token.ctxLabel')}</dt>
              <dd className="text-stone-700 dark:text-neutral-200">
                {contextPct}% ({fmt(contextUsed)}/{fmt(contextWindow)})
              </dd>
            </div>
            <div className="flex justify-between gap-3">
              <dt>{t('token.costLabel')}</dt>
              <dd className="text-stone-700 dark:text-neutral-200">{fmtUsd(costUsd)}</dd>
            </div>
          </dl>
          <div className="mt-2 border-t border-stone-100 pt-1.5 dark:border-neutral-700">
            <div className="mb-1 font-semibold text-stone-700 dark:text-neutral-200">
              {t('token.subAgentsHeading')}
            </div>
            {subAgents.length === 0 ? (
              <div className="text-stone-400 dark:text-neutral-500">{t('token.noSubAgents')}</div>
            ) : (
              <ul className="space-y-0.5 font-mono text-stone-500 dark:text-neutral-400">
                {subAgents.map(sub => (
                  <li key={sub.agentId} className="flex items-center justify-between gap-3">
                    <span className="truncate text-stone-600 dark:text-neutral-300" title={sub.agentId}>
                      {sub.agentId}
                    </span>
                    <span className="whitespace-nowrap text-stone-700 dark:text-neutral-200">
                      {fmt(sub.inputTokens + sub.outputTokens)} · {fmtUsd(sub.costUsd)} ·{' '}
                      {sub.runs}×
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
