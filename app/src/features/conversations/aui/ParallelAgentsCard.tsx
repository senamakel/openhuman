'use client';

/**
 * `spawn_parallel_agents` toolkit entry (`aui/toolkit.tsx`): renders the
 * vendored `SubagentList` element above the individual `TaskCard` rows for
 * every worker the call fanned out.
 *
 * The core sends one `spawn_parallel_agents` tool-call part plus one
 * `subagent:*` timeline row per worker task, each carrying
 * `subagent.parentCallId === <this call's toolCallId>`
 * (`SubagentProgressDetail.parent_call_id` — see
 * `store/chatRuntimeSlice.ts#SubagentActivity.parentCallId` and
 * `selectSubagentChildrenByParentCallId`). Workers are NOT part of this call's
 * own `messages`/nested transcript the way a single `task` delegation is —
 * they are independent timeline rows, read here straight from the thread's
 * live tool timeline.
 */
import type { ToolCallMessagePartComponent } from '@assistant-ui/react';

import {
  type SubagentItem,
  SubagentList,
} from '../../../components/assistant-ui/elements/subagent-list';
import { useT } from '../../../lib/i18n/I18nContext';
import { useAuiThreadId } from '../../../providers/AssistantUiRuntimeProvider';
import {
  isActiveTimelineStatus,
  selectSubagentChildrenByParentCallId,
} from '../../../store/chatRuntimeSlice';
import { useAppSelector } from '../../../store/hooks';
import { SubagentActivityCard } from './SubagentActivityCard';

const EMPTY_TIMELINE: never[] = [];

function childName(entry: ReturnType<typeof selectSubagentChildrenByParentCallId>[number]): string {
  const sub = entry.subagent;
  return (
    (sub?.displayName && sub.displayName.trim()) || sub?.agentId || entry.displayName || 'sub-agent'
  );
}

function childProgressPct(
  entry: ReturnType<typeof selectSubagentChildrenByParentCallId>[number]
): number {
  const sub = entry.subagent;
  if (!sub) return entry.status === 'running' ? 50 : 100;
  if (!isActiveTimelineStatus(sub.status ?? entry.status)) return 100;
  if (
    typeof sub.childIteration === 'number' &&
    typeof sub.childMaxIterations === 'number' &&
    sub.childMaxIterations > 0
  ) {
    return Math.max(
      0,
      Math.min(100, Math.round((sub.childIteration / sub.childMaxIterations) * 100))
    );
  }
  return 50;
}

function isFailedChild(status: string | undefined): boolean {
  return status === 'error' || status === 'failed' || status === 'cancelled';
}

function isIncompleteChild(status: string | undefined): boolean {
  return status === 'incomplete';
}

/** Adapt a `spawn_parallel_agents` tool-call part onto `SubagentList` + per-child `TaskCard` rows. */
export const ParallelAgentsCard: ToolCallMessagePartComponent = ({ toolCallId, status }) => {
  const { t } = useT();
  const threadId = useAuiThreadId();
  const timeline = useAppSelector(state =>
    threadId ? (state.chatRuntime.toolTimelineByThread[threadId] ?? EMPTY_TIMELINE) : EMPTY_TIMELINE
  );
  const children = selectSubagentChildrenByParentCallId(timeline, toolCallId);

  if (children.length === 0) return null;

  const childStatus = (entry: (typeof children)[number]) => entry.subagent?.status ?? entry.status;
  const agents: SubagentItem[] = children.map(entry => ({
    name: childName(entry),
    model: '',
    done: !isActiveTimelineStatus(childStatus(entry)),
  }));
  const runningCount = agents.filter(agent => !agent.done).length;
  const incompleteCount = children.filter(entry => isIncompleteChild(childStatus(entry))).length;
  // An incomplete worker has settled but did not finish: it is neither running
  // nor complete, so the completed total leaves it out.
  const completedCount = children.length - runningCount - incompleteCount;
  const failedCount = children.filter(entry => isFailedChild(childStatus(entry))).length;
  const progress = children.map(childProgressPct);
  const anyRunning = runningCount > 0;
  // Every worker is back but the parent call has not settled: it is reading
  // their results, which used to look like the card had simply stalled.
  const aggregating = !anyRunning && status?.type === 'running';
  const counts = [
    runningCount > 0
      ? t('chat.subagents.runningCount').replace('{count}', String(runningCount))
      : null,
    failedCount > 0
      ? t('chat.subagents.failedCount').replace('{count}', String(failedCount))
      : null,
    incompleteCount > 0
      ? t('chat.subagents.incompleteCount').replace('{count}', String(incompleteCount))
      : null,
  ].filter(Boolean);

  return (
    <div className="flex flex-col gap-2" data-testid="assistant-ui-parallel-agents-call">
      <div
        data-testid="parallel-agents-header"
        className="flex items-baseline justify-between gap-2 text-[12.5px]">
        <span className="text-content-secondary font-medium">
          {t('chat.subagents.ofTotal')
            .replace('{complete}', String(completedCount))
            .replace('{total}', String(children.length))}
        </span>
        {counts.length > 0 && (
          <span className="text-content-muted tabular-nums">{counts.join(' · ')}</span>
        )}
      </div>
      {aggregating && (
        <p data-testid="parallel-agents-aggregating" className="text-content-muted text-[12px]">
          {t('chat.subagents.settled')}
        </p>
      )}
      <SubagentList
        agents={agents}
        completedCount={completedCount}
        progress={progress}
        showSummary={anyRunning}
        summaryAgent={{ name: t('conversations.tools.parallelAgentsAggregating'), model: '' }}
      />
      <div className="flex flex-col gap-2">
        {children.map(entry =>
          entry.subagent ? <SubagentActivityCard key={entry.id} activity={entry.subagent} /> : null
        )}
      </div>
    </div>
  );
};

export default ParallelAgentsCard;
