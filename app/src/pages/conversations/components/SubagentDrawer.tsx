import { useEffect } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import type { SubagentActivity, ToolTimelineEntryStatus } from '../../../store/chatRuntimeSlice';
import { BubbleMarkdown } from './AgentMessageBubble';

/**
 * Map a subagent row's terminal/running status to the visual tone used
 * across the drawer (header dot, status pill). Mirrors the colour
 * language of `ToolTimelineBlock` so the inline card and the drawer read
 * as the same surface.
 */
function statusTone(status: ToolTimelineEntryStatus | undefined): {
  dot: string;
  pill: string;
  label: 'statusRunning' | 'statusCompleted' | 'statusFailed';
} {
  if (status === 'success') {
    return {
      dot: 'bg-sage-500',
      pill: 'bg-sage-100 dark:bg-sage-500/20 text-sage-700 dark:text-sage-300',
      label: 'statusCompleted',
    };
  }
  if (status === 'error') {
    return {
      dot: 'bg-coral-500',
      pill: 'bg-coral-100 dark:bg-coral-500/20 text-coral-700 dark:text-coral-300',
      label: 'statusFailed',
    };
  }
  return {
    dot: 'bg-amber-500 animate-pulse',
    pill: 'bg-amber-100 dark:bg-amber-500/20 text-amber-700 dark:text-amber-300',
    label: 'statusRunning',
  };
}

function formatElapsed(ms: number): string {
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
}

/**
 * Full live-transcript view for one sub-agent, slid in from the right.
 *
 * Driven entirely off the live [`SubagentActivity`] the caller passes —
 * because the caller re-derives that object from Redux on every render,
 * the drawer updates token-by-token as `subagent_text_delta` /
 * `subagent_thinking_delta` events stream in. Shows the streamed
 * reasoning (collapsible), the streamed visible output (rendered as
 * Markdown), and the chronological list of child tool calls with their
 * status and timings.
 *
 * Rendered as `null` when no subagent is selected, so the parent can
 * mount it unconditionally and just flip `subagent`.
 */
export function SubagentDrawer({
  subagent,
  status,
  onClose,
}: {
  subagent: SubagentActivity | null;
  /** Lifecycle status of the owning timeline row (running/success/error). */
  status?: ToolTimelineEntryStatus;
  onClose: () => void;
}) {
  const { t } = useT();

  // Close on Escape for keyboard parity with the backdrop click.
  useEffect(() => {
    if (!subagent) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [subagent, onClose]);

  if (!subagent) return null;

  const tone = statusTone(status);
  const thinking = subagent.streamingThinking ?? '';
  const output = subagent.streamingText ?? '';
  const hasOutput = output.trim().length > 0;
  const isRunning = status !== 'success' && status !== 'error';

  return (
    <div className="fixed inset-0 z-50 flex justify-end" data-testid="subagent-drawer">
      {/* Backdrop */}
      <button
        type="button"
        aria-label={t('conversations.subagent.close')}
        className="absolute inset-0 bg-stone-900/30 dark:bg-black/50"
        onClick={onClose}
      />
      <aside className="relative flex h-full w-full max-w-md flex-col bg-white dark:bg-neutral-900 shadow-xl">
        {/* Header */}
        <header className="flex items-center gap-2.5 border-b border-stone-200 dark:border-neutral-800 px-4 py-3">
          <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-ocean-50 dark:bg-ocean-500/15 text-base">
            🤖
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="truncate font-semibold text-stone-800 dark:text-neutral-100">
                {subagent.agentId}
              </span>
              <span className={`h-2 w-2 shrink-0 rounded-full ${tone.dot}`} />
            </div>
            <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-stone-500 dark:text-neutral-400">
              <span className={`rounded-full px-1.5 py-0.5 ${tone.pill}`}>
                {t(`conversations.subagent.${tone.label}`)}
              </span>
              {subagent.childIteration != null && subagent.childMaxIterations != null ? (
                <span>
                  {t('conversations.toolTimeline.turn')} {subagent.childIteration}/
                  {subagent.childMaxIterations}
                </span>
              ) : subagent.iterations != null ? (
                <span>
                  {subagent.iterations} {t('conversations.toolTimeline.turn')}
                </span>
              ) : null}
              {subagent.elapsedMs != null ? <span>{formatElapsed(subagent.elapsedMs)}</span> : null}
              {subagent.mode ? <span>{subagent.mode}</span> : null}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={t('conversations.subagent.close')}
            className="shrink-0 rounded-full p-1.5 text-stone-400 hover:bg-stone-100 dark:hover:bg-neutral-800 hover:text-stone-600 dark:hover:text-neutral-200">
            ✕
          </button>
        </header>

        {/* Body */}
        <div className="flex-1 space-y-4 overflow-y-auto px-4 py-4">
          {/* Streamed reasoning */}
          {thinking.trim().length > 0 ? (
            <section>
              <h3 className="mb-1.5 flex items-center gap-1.5 text-xs font-semibold text-stone-500 dark:text-neutral-400">
                <span className="inline-block h-1.5 w-1.5 rounded-full bg-primary-400" />
                {t('conversations.subagent.thinking')}
              </h3>
              <pre className="whitespace-pre-wrap break-words rounded-lg bg-stone-50 dark:bg-neutral-800/60 px-3 py-2 font-sans text-[12px] leading-relaxed text-stone-600 dark:text-neutral-300">
                {thinking}
              </pre>
            </section>
          ) : null}

          {/* Streamed visible output */}
          <section>
            <h3 className="mb-1.5 text-xs font-semibold text-stone-500 dark:text-neutral-400">
              {t('conversations.subagent.response')}
            </h3>
            {hasOutput ? (
              <div className="rounded-lg bg-stone-50 dark:bg-neutral-800/60 px-3 py-2">
                <BubbleMarkdown content={output} />
                {isRunning ? (
                  <span className="ml-0.5 inline-block h-3 w-1 animate-pulse bg-primary-400 align-middle" />
                ) : null}
              </div>
            ) : (
              <p className="text-xs italic text-stone-400 dark:text-neutral-500">
                {isRunning
                  ? t('conversations.subagent.working')
                  : t('conversations.subagent.noOutputYet')}
              </p>
            )}
          </section>

          {/* Child tool calls */}
          {subagent.toolCalls.length > 0 ? (
            <section>
              <h3 className="mb-1.5 text-xs font-semibold text-stone-500 dark:text-neutral-400">
                {t('conversations.subagent.toolCalls')} ({subagent.toolCalls.length})
              </h3>
              <ul className="space-y-1">
                {subagent.toolCalls.map(call => {
                  const callTone =
                    call.status === 'running'
                      ? 'text-amber-700 dark:text-amber-300'
                      : call.status === 'success'
                        ? 'text-sage-700 dark:text-sage-300'
                        : 'text-coral-700 dark:text-coral-300';
                  return (
                    <li
                      key={call.callId}
                      className="flex items-center gap-2 rounded-md bg-stone-50 dark:bg-neutral-800/60 px-2.5 py-1.5 text-xs"
                      data-testid="subagent-drawer-tool-call">
                      <span className={callTone}>●</span>
                      <span className="font-mono text-stone-700 dark:text-neutral-200">
                        {call.toolName}
                      </span>
                      {call.iteration != null ? (
                        <span className="text-[10px] text-stone-400 dark:text-neutral-500">
                          t{call.iteration}
                        </span>
                      ) : null}
                      <span className={`ml-auto ${callTone}`}>{call.status}</span>
                      {call.elapsedMs != null && call.status !== 'running' ? (
                        <span className="text-[10px] text-stone-400 dark:text-neutral-500">
                          {formatElapsed(call.elapsedMs)}
                        </span>
                      ) : null}
                    </li>
                  );
                })}
              </ul>
            </section>
          ) : null}
        </div>
      </aside>
    </div>
  );
}
