import React, { useCallback, useEffect, useRef, useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import {
  type ThreadGoal,
  threadGoalApi,
  type ThreadGoalStatus,
} from '../../../services/api/threadGoalApi';

/**
 * Compact chip surfacing the *current thread's* goal — a Codex-style per-thread
 * completion contract the agent pursues across turns. Pinned above the composer
 * next to the {@link ThreadTodoStrip} (which is the thread's task board). This is
 * distinct from the global long-term goals list on the Intelligence tab.
 *
 * The agent sets/refines the goal itself (via `goal_set`); this chip lets the
 * user see status + token budget and set / edit / pause / resume / complete /
 * clear it directly. Liveness: it fetches on thread change and polls on a light
 * interval so agent- and continuation-driven changes surface without a manual
 * refresh. (A push channel can replace the poll later.)
 */

const POLL_INTERVAL_MS = 10_000;

/** Tailwind classes per status, using the app's ocean/sage/amber/coral palette. */
function statusClasses(status: ThreadGoalStatus): string {
  switch (status) {
    case 'active':
      return 'bg-primary-50 text-primary-700 dark:bg-primary-900/40 dark:text-primary-200';
    case 'paused':
      return 'bg-stone-100 text-stone-600 dark:bg-neutral-800 dark:text-neutral-300';
    case 'budget_limited':
      return 'bg-amber-50 text-amber-700 dark:bg-amber-900/40 dark:text-amber-200';
    case 'complete':
      return 'bg-sage-50 text-sage-700 dark:bg-sage-900/40 dark:text-sage-200';
    default:
      return 'bg-stone-100 text-stone-600';
  }
}

interface Props {
  threadId: string;
  /** Test seam: inject a stub client. Defaults to the real {@link threadGoalApi}. */
  api?: typeof threadGoalApi;
}

export function ThreadGoalChip({
  threadId,
  api = threadGoalApi,
}: Props): React.ReactElement | null {
  const { t } = useT();
  const [goal, setGoal] = useState<ThreadGoal | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  // Avoid setState after unmount / thread switch races.
  const activeThread = useRef(threadId);

  const refresh = useCallback(async () => {
    try {
      const g = await api.get(threadId);
      if (activeThread.current === threadId) setGoal(g);
    } catch {
      /* best-effort; keep last known goal */
    }
  }, [api, threadId]);

  // Fetch on mount and poll lightly. The parent remounts this component per
  // thread (`key={threadId}`), so a thread switch resets state via a fresh
  // mount rather than synchronous setState here.
  useEffect(() => {
    activeThread.current = threadId;
    // Fire-and-forget fetch: setState lands in a later microtask, not
    // synchronously. Matches the codebase's fetch-on-mount precedent.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh();
    const id = window.setInterval(() => void refresh(), POLL_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [threadId, refresh]);

  const runAction = useCallback(
    async (fn: () => Promise<ThreadGoal | null | boolean>) => {
      setBusy(true);
      try {
        await fn();
        await refresh();
      } finally {
        setBusy(false);
      }
    },
    [refresh]
  );

  const saveDraft = useCallback(() => {
    const objective = draft.trim();
    if (!objective) return;
    setEditing(false);
    void runAction(() => api.set(threadId, objective));
  }, [api, draft, runAction, threadId]);

  const beginEdit = useCallback(() => {
    setDraft(goal?.objective ?? '');
    setEditing(true);
  }, [goal]);

  if (!threadId) return null;

  // Editing form (set or edit).
  if (editing) {
    return (
      <div className="mb-2 flex items-center gap-2 rounded-md border border-stone-200 bg-white/60 px-2 py-1.5 dark:border-neutral-700 dark:bg-neutral-900/60">
        <span className="shrink-0 text-xs font-medium text-stone-500 dark:text-neutral-400">
          {t('conversations.threadGoal.label')}
        </span>
        <input
          autoFocus
          value={draft}
          onChange={e => setDraft(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter') saveDraft();
            if (e.key === 'Escape') setEditing(false);
          }}
          placeholder={t('conversations.threadGoal.placeholder')}
          aria-label={t('conversations.threadGoal.placeholder')}
          className="min-w-0 flex-1 bg-transparent text-sm text-stone-800 outline-none placeholder:text-stone-400 dark:text-neutral-100"
        />
        <button
          type="button"
          onClick={saveDraft}
          disabled={!draft.trim()}
          className="shrink-0 rounded px-2 py-0.5 text-xs font-medium text-primary-600 hover:bg-primary-50 disabled:opacity-40 dark:text-primary-300 dark:hover:bg-primary-900/40">
          {t('conversations.threadGoal.save')}
        </button>
        <button
          type="button"
          onClick={() => setEditing(false)}
          className="shrink-0 rounded px-2 py-0.5 text-xs text-stone-500 hover:bg-stone-100 dark:text-neutral-400 dark:hover:bg-neutral-800">
          {t('conversations.threadGoal.cancel')}
        </button>
      </div>
    );
  }

  // No goal yet → subtle "Set goal" affordance.
  if (!goal) {
    return (
      <div className="mb-2">
        <button
          type="button"
          onClick={beginEdit}
          className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs text-stone-500 hover:bg-stone-100 dark:text-neutral-400 dark:hover:bg-neutral-800">
          <span aria-hidden>◎</span>
          {t('conversations.threadGoal.setCta')}
        </button>
      </div>
    );
  }

  const budgetText =
    typeof goal.tokenBudget === 'number' && goal.tokenBudget > 0
      ? `${goal.tokensUsed.toLocaleString()} / ${goal.tokenBudget.toLocaleString()} ${t('conversations.threadGoal.tokensSuffix')}`
      : null;

  return (
    <div className="mb-2 flex items-center gap-2 rounded-md border border-stone-200 bg-white/60 px-2 py-1.5 text-sm dark:border-neutral-700 dark:bg-neutral-900/60">
      <span aria-hidden className="shrink-0 text-stone-400">
        ◎
      </span>
      <span
        className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide ${statusClasses(goal.status)}`}>
        {t(`conversations.threadGoal.status.${goal.status}`)}
      </span>
      <span
        className="min-w-0 flex-1 truncate text-stone-800 dark:text-neutral-100"
        title={goal.objective}>
        {goal.objective}
      </span>
      {budgetText && (
        <span className="shrink-0 text-[11px] tabular-nums text-stone-400 dark:text-neutral-500">
          {budgetText}
        </span>
      )}
      <div className="flex shrink-0 items-center gap-0.5">
        {goal.status === 'active' && (
          <ChipButton
            label={t('conversations.threadGoal.pause')}
            disabled={busy}
            onClick={() => void runAction(() => api.pause(threadId))}
          />
        )}
        {goal.status === 'paused' && (
          <ChipButton
            label={t('conversations.threadGoal.resume')}
            disabled={busy}
            onClick={() => void runAction(() => api.resume(threadId))}
          />
        )}
        {goal.status !== 'complete' && (
          <ChipButton
            label={t('conversations.threadGoal.complete')}
            disabled={busy}
            onClick={() => void runAction(() => api.complete(threadId))}
          />
        )}
        <ChipButton
          label={t('conversations.threadGoal.edit')}
          disabled={busy}
          onClick={beginEdit}
        />
        <ChipButton
          label={t('conversations.threadGoal.clear')}
          disabled={busy}
          onClick={() => void runAction(() => api.clear(threadId))}
        />
      </div>
    </div>
  );
}

function ChipButton({
  label,
  onClick,
  disabled,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
}): React.ReactElement {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      disabled={disabled}
      className="rounded px-1.5 py-0.5 text-[11px] text-stone-500 hover:bg-stone-100 disabled:opacity-40 dark:text-neutral-400 dark:hover:bg-neutral-800">
      {label}
    </button>
  );
}

export default ThreadGoalChip;
