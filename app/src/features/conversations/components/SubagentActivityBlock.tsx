import Badge, { type BadgeVariant } from '../../../components/ui/Badge';
import WorktreeActions from '../../../components/worktree/WorktreeActions';
import { useT } from '../../../lib/i18n/I18nContext';
import type {
  SubagentActivity,
  ToolFailureExplanation,
  ToolTimelineEntryStatus,
} from '../../../store/chatRuntimeSlice';
import { basename } from '../../../utils/pathUtils';
import { formatToolName, stripToolCallEnvelopes } from '../../../utils/toolTimelineFormatting';
import { BubbleMarkdown } from './AgentMessageBubble';
import { ToolFailureLines } from './ToolFailureLines';

/**
 * Map a child tool-call status to the shared {@link Badge} variant. The label
 * itself comes from `useStatusTagLabel` below; keeping the two beside each
 * other is what stops a status reading "Failed" in a success-toned pill.
 */
function statusTagVariant(status: ToolTimelineEntryStatus): BadgeVariant {
  switch (status) {
    case 'error':
      return 'danger';
    case 'running':
    case 'awaiting_user':
      return 'warning';
    case 'cancelled':
      return 'neutral';
    default:
      return 'success';
  }
}

/** Tone classes for a child tool-call row's bullet, keyed by lifecycle status. */
function toolCallTone(status: ToolTimelineEntryStatus): string {
  if (status === 'running') return 'text-amber-700 dark:text-amber-300';
  if (status === 'success') return 'text-sage-700 dark:text-sage-300';
  return 'text-coral-700 dark:text-coral-300';
}

/**
 * Status pill for a tool-call row — a tinted "Done" / "Failed" / "Running"
 * tag instead of a bare ✓/✕ glyph, so the outcome reads at a glance. Built on
 * the shared {@link Badge} primitive, which publishes the tone as
 * `data-variant` for tests to assert instead of a Tailwind class string.
 */
export function StatusTag({ status }: { status: ToolTimelineEntryStatus }) {
  const { t } = useT();
  const label =
    status === 'error'
      ? t('conversations.agentTaskInsights.failed')
      : status === 'running'
        ? t('conversations.agentTaskInsights.running')
        : status === 'cancelled'
          ? t('conversations.agentTaskInsights.cancelled')
          : status === 'awaiting_user'
            ? t('conversations.agentTaskInsights.awaitingUser')
            : t('conversations.agentTaskInsights.done');
  return (
    <Badge variant={statusTagVariant(status)} className="rounded-full px-1.5 py-0.5 text-[10px]">
      {label}
    </Badge>
  );
}

/**
 * One child tool-call row in a sub-agent's inline activity. Shared by the
 * ordered transcript (interleaved with {@link ThoughtBlock}) and the flat
 * `toolCalls` fallback, so the row markup lives in exactly one place.
 */
export function ToolCallRow({
  call,
}: {
  call: {
    callId: string;
    toolName: string;
    status: ToolTimelineEntryStatus;
    elapsedMs?: number;
    iteration?: number;
    /** Server-computed human label; preferred over the client formatter. */
    displayName?: string;
    /** Server-computed contextual detail (path / recipient / query). */
    detail?: string;
    /** Structured why/next explanation for a FAILED child tool call (#4459). */
    failure?: ToolFailureExplanation;
  };
}) {
  return (
    <div className="min-w-0" data-testid="subagent-tool-call">
      <div className="flex min-w-0 items-center gap-1.5">
        <span aria-hidden className={`shrink-0 text-[11px] ${toolCallTone(call.status)}`}>
          •
        </span>
        <span className="shrink-0 text-[12px] whitespace-nowrap text-content-secondary">
          {call.displayName ?? formatToolName(call.toolName)}
        </span>
        {/* The contextual arg (path / recipient / query) can be long, so it
          truncates to a single line and absorbs the row's spare width — the
          full value stays available on hover — instead of wrapping into a
          multi-line box that knocks the name and status out of alignment. */}
        {call.detail ? (
          <span
            title={call.detail}
            className="min-w-0 truncate rounded bg-surface-subtle px-1 py-px font-mono text-[11px] text-content-muted">
            {call.detail}
          </span>
        ) : null}
        {/* Status reads as a tinted "Done" / "Failed" / "Running" tag. */}
        <span className="shrink-0">
          <StatusTag status={call.status} />
        </span>
        {call.elapsedMs != null && call.status !== 'running' ? (
          <span className="shrink-0 text-[11px] text-content-faint">
            {call.elapsedMs >= 1000
              ? `${(call.elapsedMs / 1000).toFixed(1)}s`
              : `${call.elapsedMs}ms`}
          </span>
        ) : null}
      </div>
      {call.status === 'error' && call.failure ? <ToolFailureLines failure={call.failure} /> : null}
    </div>
  );
}

/**
 * The agent's reasoning or visible narration, surfaced inline in the timeline
 * as quoted/italic prose at the position it streamed — so a thought shows up
 * wherever it occurred between tool calls. Shown directly (no "Thoughts"
 * heading, no collapse). Both `thinking` and `text` transcript items render
 * through here. Renders nothing for an all-whitespace delta so a half-streamed
 * item never flashes an empty quote.
 */
export function ThoughtBlock({ text }: { text: string }) {
  // Drop any inline `<tool_call>…</tool_call>` envelope the model emitted as
  // text — the call already shows as its own row. Keep the original newlines
  // (only trim the ends) so the markdown renderer can see headings, lists,
  // code fences and emphasis instead of flattening them to one plain line.
  const clean = stripToolCallEnvelopes(text).trim();
  if (!clean) return null;
  // Rendered through the shared `BubbleMarkdown` so a thought formats markdown
  // (bold, code, lists) — but scaled back to the original quiet thought look:
  // small (12px) and light/muted, not the larger, darker agent-bubble prose.
  // Descendant overrides on `.prose` beat the typography plugin's base sizing;
  // code keeps its accent colour so inline `tool_names` still read clearly.
  return (
    <div
      data-testid="subagent-thought"
      className="my-0.5 wrap-break-word [&_.prose]:text-[12px] [&_.prose]:leading-relaxed [&_.prose]:text-content-muted [&_.prose_strong]:text-content-muted [&_.prose_:is(h1,h2,h3,h4,h5,h6)]:text-[12px] [&_.prose_:is(h1,h2,h3,h4,h5,h6)]:text-content-muted">
      <BubbleMarkdown content={clean} />
    </div>
  );
}

/**
 * Render the live activity of one running (or completed) sub-agent inside its
 * parent timeline row — the mode/dedicated-thread badge, the child iteration
 * counter, the final-run statistics, and the ordered transcript of child tool
 * calls interleaved with the agent's "Thoughts" (reasoning + narration).
 *
 * Kept as a sibling of the existing worker-thread / detail block so the
 * surrounding disclosure chevron + status pill behaviour is unaffected — this
 * component only renders when `subagent` is present on the entry, which is true
 * for any row produced by the `subagent_*` socket events from a current core.
 */
export function SubagentActivityBlock({
  subagent,
  onView,
}: {
  subagent: SubagentActivity;
  /** Opens the full-transcript drawer for this subagent. Omitted in
   * read-only contexts (e.g. a completed snapshot with no live driver). */
  onView?: () => void;
}) {
  const { t } = useT();
  const headerBits: string[] = [];
  if (subagent.mode) headerBits.push(subagent.mode);
  if (subagent.dedicatedThread) headerBits.push(t('conversations.toolTimeline.workerThread'));
  if (subagent.childIteration != null) {
    if (subagent.childMaxIterations != null) {
      headerBits.push(
        `${t('conversations.toolTimeline.turn')} ${subagent.childIteration}/${subagent.childMaxIterations}`
      );
    } else {
      headerBits.push(`${t('conversations.toolTimeline.step')} ${subagent.childIteration}`);
    }
  } else if (subagent.iterations != null) {
    headerBits.push(
      subagent.iterations === 1
        ? `${subagent.iterations} ${t('chat.turn')}`
        : `${subagent.iterations} ${t('chat.turns')}`
    );
  }
  if (subagent.elapsedMs != null) {
    headerBits.push(
      subagent.elapsedMs >= 1000
        ? `${(subagent.elapsedMs / 1000).toFixed(1)}s`
        : `${subagent.elapsedMs}ms`
    );
  }

  // The ordered transcript drives the inline activity: child tool-call rows
  // and the agent's "Thoughts" (reasoning + visible narration) render in the
  // exact order they streamed, so each thought appears wherever it occurred
  // between tool calls. Falls back to the flat tool-call list when the prose
  // transcript is absent (e.g. a rehydrated/interrupted snapshot).
  const transcript = subagent.transcript ?? [];

  return (
    <div
      className="mt-1 space-y-0.5 text-[12px] text-content-muted"
      data-testid="subagent-activity">
      {headerBits.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5">
          {headerBits.map(bit => (
            <Badge key={bit} className="rounded-full">
              {bit}
            </Badge>
          ))}
        </div>
      ) : null}
      {transcript.length > 0 ? (
        <div className="ml-1 space-y-0.5" data-testid="subagent-transcript">
          {transcript.map((item, i) =>
            item.kind === 'tool' ? (
              <ToolCallRow key={item.callId} call={item} />
            ) : (
              <ThoughtBlock key={`thought-${i}`} text={item.text} />
            )
          )}
        </div>
      ) : subagent.toolCalls.length > 0 ? (
        <div className="ml-1 space-y-0.5">
          {subagent.toolCalls.map(call => (
            <ToolCallRow key={call.callId} call={call} />
          ))}
        </div>
      ) : null}
      {subagent.worktreePath ? (
        <div
          className="mt-1 space-y-1 rounded-md border border-line bg-surface-muted/70 p-1.5"
          data-testid="subagent-worktree">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="font-medium text-content-secondary">{t('worktree.label')}</span>
            <span
              className="truncate font-mono text-[12px] text-content-muted"
              title={subagent.worktreePath}>
              {basename(subagent.worktreePath)}
            </span>
            <Badge variant={subagent.isDirty ? 'warning' : 'success'} className="rounded-full">
              {subagent.isDirty ? t('worktree.dirty') : t('worktree.clean')}
            </Badge>
            {subagent.changedFiles && subagent.changedFiles.length > 0 ? (
              <span className="text-[11px] text-content-faint">
                {subagent.changedFiles.length}{' '}
                {subagent.changedFiles.length === 1
                  ? t('worktree.changedFile')
                  : t('worktree.changedFiles')}
              </span>
            ) : null}
          </div>
          <WorktreeActions path={subagent.worktreePath} isDirty={subagent.isDirty} compact />
        </div>
      ) : null}
      {onView ? (
        <button
          type="button"
          onClick={onView}
          data-testid="subagent-view-processing"
          className="mt-0.5 rounded-full px-1.5 py-0.5 text-[12px] font-medium text-primary-600 hover:bg-primary-50 dark:text-primary-300 dark:hover:bg-primary-500/15">
          {t('conversations.subagent.viewProcessing')} →
        </button>
      ) : null}
    </div>
  );
}
