import createDebug from 'debug';
import { useState } from 'react';

import Badge, { type BadgeVariant } from '../../../components/ui/Badge';
import {
  CollapsibleContent,
  CollapsibleRoot,
  CollapsibleTrigger,
} from '../../../components/ui/Collapsible';
import { useT } from '../../../lib/i18n/I18nContext';
import type {
  SubagentTranscriptItem,
  ToolTimelineEntryStatus,
} from '../../../store/chatRuntimeSlice';
import { ToolFailureLines } from './ToolFailureLines';

const log = createDebug('app:conversations:subagent-tool-call');

/** Human-readable elapsed time for a sub-agent run or one of its tool calls. */
export function formatElapsed(ms: number): string {
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
}

/**
 * Map a sub-agent lifecycle status to the shared {@link Badge} variant used
 * for it everywhere in the conversation panel, so the drawer header pill, the
 * transcript tool rows and the inline timeline all read as one system. Kept in
 * one place because a status that renders `warning` in one surface and
 * `neutral` in another is a bug nobody notices until a screenshot diff.
 */
export function subagentStatusVariant(status: ToolTimelineEntryStatus | undefined): BadgeVariant {
  switch (status) {
    case 'success':
      return 'success';
    case 'error':
      return 'danger';
    case 'cancelled':
      return 'neutral';
    default:
      // running / awaiting_user — in flight, needs attention.
      return 'warning';
  }
}

/** Localised label for a sub-agent lifecycle status. */
export function useSubagentStatusLabel(status: ToolTimelineEntryStatus | undefined): string {
  const { t } = useT();
  switch (status) {
    case 'success':
      return t('conversations.subagent.statusCompleted');
    case 'error':
      return t('conversations.subagent.statusFailed');
    case 'cancelled':
      return t('conversations.subagent.statusCancelled');
    case 'awaiting_user':
      return t('conversations.subagent.statusAwaitingUser');
    default:
      return t('conversations.subagent.statusRunning');
  }
}

type SubagentToolItem = Extract<SubagentTranscriptItem, { kind: 'tool' }>;

/**
 * Pretty-print a tool's input arguments for display. Objects/arrays are
 * rendered as indented JSON; a string is shown verbatim. Returns `null` when
 * there are no arguments to show (e.g. a tool called with no input, or a
 * transcript reopened from memory where args weren't persisted).
 */
export function formatArgs(args: unknown): string | null {
  if (args == null) return null;
  if (typeof args === 'string') return args.length > 0 ? args : null;
  try {
    return JSON.stringify(args, null, 2);
  } catch {
    return String(args);
  }
}

const DETAIL_PRE =
  'max-h-60 overflow-auto whitespace-pre-wrap wrap-break-word rounded bg-surface px-2 py-1.5 ' +
  'font-mono text-[11px] leading-relaxed text-content-secondary';
const DETAIL_LABEL = 'mb-1 text-[10px] font-semibold uppercase tracking-wide text-content-faint';

/**
 * One child tool call in the drawer transcript, expandable to reveal exactly
 * *what happened*: the input arguments the sub-agent passed and the raw output
 * the tool returned. Collapsed by default to keep the transcript scannable;
 * the disclosure is only enabled once there's detail to reveal (args present,
 * or the call completed with a captured result). Reopened-from-memory
 * transcripts carry no args/result, so those rows stay non-expandable.
 *
 * The disclosure is the shared Radix {@link CollapsibleRoot} rather than a
 * hand-rolled `useState` + conditional render, so the trigger carries real
 * `aria-expanded` / `aria-controls` wiring instead of an ad-hoc attribute.
 */
export function SubagentToolCallRow({ item }: { item: SubagentToolItem }) {
  const { t } = useT();
  const [expanded, setExpanded] = useState(false);

  const statusLabel = useSubagentStatusLabel(item.status);
  const argsText = formatArgs(item.args);
  const hasOutput = item.result != null;
  const expandable = argsText != null || hasOutput;

  return (
    <CollapsibleRoot
      open={expandable && expanded}
      onOpenChange={next => {
        log('tool-call %s expanded=%s', item.toolName, next);
        setExpanded(next);
      }}
      className="rounded-md border border-line bg-surface-muted text-xs"
      data-testid="subagent-drawer-tool-call">
      <CollapsibleTrigger
        size="sm"
        disabled={!expandable}
        data-testid="subagent-tool-call-toggle"
        className="justify-start gap-2 px-2.5 py-1.5 font-normal hover:bg-transparent disabled:cursor-default disabled:opacity-100">
        {expandable ? (
          <span aria-hidden className="shrink-0 text-[9px] text-content-faint">
            {expanded ? '▾' : '▸'}
          </span>
        ) : (
          <span className="w-[9px] shrink-0" aria-hidden />
        )}
        <span aria-hidden>🔧</span>
        <span className="font-mono text-content-secondary">{item.toolName}</span>
        <Badge variant={subagentStatusVariant(item.status)} className="ml-auto shrink-0">
          {statusLabel}
        </Badge>
        {item.elapsedMs != null && item.status !== 'running' ? (
          <span className="shrink-0 text-[10px] text-content-faint">
            {formatElapsed(item.elapsedMs)}
          </span>
        ) : null}
      </CollapsibleTrigger>
      {item.status === 'error' && item.failure ? (
        <div className="px-2.5 pb-1.5">
          <ToolFailureLines failure={item.failure} />
        </div>
      ) : null}
      <CollapsibleContent size="sm" className="space-y-2 border-t border-line px-2.5 py-2 pb-2">
        {argsText != null ? (
          <div data-testid="subagent-tool-call-input">
            <div className={DETAIL_LABEL}>{t('conversations.subagent.input')}</div>
            <pre className={DETAIL_PRE}>{argsText}</pre>
          </div>
        ) : null}
        {hasOutput ? (
          <div data-testid="subagent-tool-call-output">
            <div className={DETAIL_LABEL}>{t('conversations.subagent.output')}</div>
            <pre className={DETAIL_PRE}>
              {item.result && item.result.length > 0
                ? item.result
                : t('conversations.subagent.noOutput')}
            </pre>
          </div>
        ) : null}
      </CollapsibleContent>
    </CollapsibleRoot>
  );
}
