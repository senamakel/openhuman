import { useEffect, useRef, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { emptySessionTokenUsage, type SubAgentUsage } from '../../store/chatRuntimeSlice';
import { useAppSelector } from '../../store/hooks';
import Tooltip from '../ui/Tooltip';

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

/**
 * One labelled row in the hover breakdown. The label carries a `title` tooltip
 * explaining the metric, hinted with a dotted underline + help cursor.
 */
function UsageRow({ label, tip, value }: { label: string; tip: string; value: string }) {
  return (
    <div className="flex justify-between gap-3">
      <dt
        title={tip}
        className="cursor-help underline decoration-dotted decoration-stone-300 underline-offset-2 dark:decoration-neutral-600">
        {label}
      </dt>
      <dd className="font-mono text-stone-700 dark:text-neutral-200">{value}</dd>
    </div>
  );
}

/** One agent's line in the per-agent breakdown: combined tokens · cost · runs. */
function AgentLine({
  name,
  tokens,
  costUsd,
  runs,
}: {
  name: string;
  tokens: number;
  costUsd: number;
  runs: number;
}) {
  return (
    <li className="flex items-center justify-between gap-3">
      <span className="truncate text-stone-600 dark:text-neutral-300" title={name}>
        {name}
      </span>
      <span className="whitespace-nowrap font-mono text-stone-700 dark:text-neutral-200">
        {fmt(tokens)} · {fmtUsd(costUsd)} · {runs}×
      </span>
    </li>
  );
}

interface ComposerTokenStatsProps {
  /** Resolved model id, surfaced inside the breakdown popover. */
  model?: string | null;
  /**
   * Active thread id. When set, the footer shows that thread's usage bucket
   * (seeded from persisted transcripts + live turns); otherwise it falls back
   * to the global app-session aggregate.
   */
  threadId?: string | null;
}

const EMPTY_USAGE = emptySessionTokenUsage();

export default function ComposerTokenStats({ model, threadId }: ComposerTokenStatsProps = {}) {
  const { t } = useT();
  const usage = useAppSelector(state =>
    threadId
      ? (state.chatRuntime.usageByThread[threadId] ?? EMPTY_USAGE)
      : state.chatRuntime.sessionTokenUsage
  );
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  // The breakdown is click-toggled (not hover). Dismiss on an outside click or
  // Escape so it behaves like a popover rather than a sticky panel.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onPointerDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const inTok = usage.inputTokens || 0;
  const outTok = usage.outputTokens || 0;
  const cachedTok = usage.cachedTokens || 0;
  const turns = usage.turns || 0;
  const costUsd = usage.costUsd || 0;
  const subAgents: SubAgentUsage[] = Object.values(usage.subAgents ?? {});

  // Render as soon as a model is resolved (even before the first turn) so the
  // clickable usage row is always present in the footer; the context segment
  // shows 0% until the first turn reports usage.
  if (turns === 0 && !model) return null;

  const contextWindow = ok(usage.contextWindow) ? usage.contextWindow : DEFAULT_CONTEXT_WINDOW;
  const contextUsed = usage.lastTurnContextUsed || 0;
  const contextPct = Math.min(100, Math.round((contextUsed / contextWindow) * 100));

  const showIo = ok(inTok) || ok(outTok);
  const showCost = ok(costUsd);

  // Orchestrator (the parent/main agent) spend = session totals minus everything
  // attributed to sub-agents. Derived here so no extra backend data is needed.
  const subTotals = subAgents.reduce(
    (acc, s) => ({
      tokens: acc.tokens + s.inputTokens + s.outputTokens,
      cost: acc.cost + s.costUsd,
    }),
    { tokens: 0, cost: 0 }
  );
  const orchestratorTokens = Math.max(0, inTok + outTok - subTotals.tokens);
  const orchestratorCost = Math.max(0, costUsd - subTotals.cost);

  const parts: React.ReactNode[] = [];

  // Inline footer: context usage · in/out tokens · cost. The context counter is
  // always shown once the footer is visible (it's the primary metric and the
  // toggle hint); the segment is highlighted while the breakdown is open.
  parts.push(
    <span
      key="ctx"
      title={t('token.contextWindow')}
      className={
        open
          ? 'rounded bg-ocean-100 px-1 text-ocean-700 dark:bg-ocean-500/20 dark:text-ocean-300'
          : undefined
      }>
      {t('token.ctxLabel')} {contextPct}% ({fmt(contextUsed)}/{fmt(contextWindow)})
    </span>
  );
  if (showIo) {
    parts.push(
      <span key="io" title={`${t('token.inputTokens')} / ${t('token.outputTokens')}`}>
        {t('token.inLabel')} {fmt(inTok)} {t('token.outLabel')} {fmt(outTok)}
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
    <div ref={rootRef} className="relative flex min-w-0 items-center">
      {/* Hover hint that the compact row is interactive; click opens the full
          breakdown. The hint is suppressed while the popover is already open. */}
      <Tooltip label={open ? '' : t('token.clickForDetails')} side="top">
        <button
          type="button"
          onClick={() => setOpen(o => !o)}
          aria-expanded={open}
          aria-label={t('token.sessionUsageTitle')}
          className="flex min-w-0 cursor-pointer flex-wrap items-center gap-2.5 border-0 bg-transparent p-0 text-[10px] font-mono text-stone-400 dark:text-neutral-500 select-none">
          {parts.map((part, i) => (
            <span key={i} className="contents">
              {i > 0 && dot()}
              {part}
            </span>
          ))}
        </button>
      </Tooltip>
      {open && (
        <div
          data-testid="composer-token-breakdown"
          role="dialog"
          aria-label={t('token.sessionUsageTitle')}
          className="absolute bottom-full left-0 z-50 mb-1.5 w-64 rounded-md border border-stone-200 bg-white p-2.5 text-[11px] shadow-lg dark:border-neutral-700 dark:bg-neutral-800">
          <div className="mb-1.5 font-semibold text-stone-700 dark:text-neutral-200">
            {t('token.sessionUsageTitle')}
          </div>
          {model && (
            <div
              className="mb-1.5 truncate font-mono text-stone-400 dark:text-neutral-500"
              title={model}>
              {model}
            </div>
          )}
          <dl className="space-y-0.5 text-stone-500 dark:text-neutral-400">
            <UsageRow label={t('token.popInput')} tip={t('token.tipInput')} value={fmt(inTok)} />
            <UsageRow label={t('token.popOutput')} tip={t('token.tipOutput')} value={fmt(outTok)} />
            {ok(cachedTok) && (
              <UsageRow
                label={t('token.popCacheHit')}
                tip={t('token.tipCacheHit')}
                value={`${fmt(cachedTok)} (${
                  inTok > 0 ? Math.min(100, Math.round((cachedTok / inTok) * 100)) : 0
                }%)`}
              />
            )}
            <UsageRow
              label={t('token.popContext')}
              tip={t('token.contextWindow')}
              value={`${contextPct}% (${fmt(contextUsed)}/${fmt(contextWindow)})`}
            />
            <UsageRow
              label={t('token.costLabel')}
              tip={t('token.costTitle')}
              value={fmtUsd(costUsd)}
            />
          </dl>
          <div className="mt-2 border-t border-stone-100 pt-1.5 dark:border-neutral-700">
            <div className="mb-1 font-semibold text-stone-700 dark:text-neutral-200">
              {t('token.byAgentHeading')}
            </div>
            <ul className="space-y-0.5 text-stone-500 dark:text-neutral-400">
              {/* Orchestrator first, then each sub-agent archetype. */}
              <AgentLine
                name={t('token.orchestrator')}
                tokens={orchestratorTokens}
                costUsd={orchestratorCost}
                runs={turns}
              />
              {subAgents.map(sub => (
                <AgentLine
                  key={sub.agentId}
                  name={sub.agentId}
                  tokens={sub.inputTokens + sub.outputTokens}
                  costUsd={sub.costUsd}
                  runs={sub.runs}
                />
              ))}
            </ul>
            {subAgents.length === 0 && (
              <div className="mt-0.5 text-stone-400 dark:text-neutral-500">
                {t('token.noSubAgents')}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
