import { useT } from '../../../lib/i18n/I18nContext';
import type {
  ProcessingTranscriptItem,
  ToolFailureExplanation,
  ToolTimelineEntry,
  ToolTimelineEntryStatus,
} from '../../../store/chatRuntimeSlice';
import {
  buildProcessingBlocks,
  categorizeTool,
  formatTimelineEntry,
  stripToolCallEnvelopes,
  type ToolCategory,
} from '../../../utils/toolTimelineFormatting';

/**
 * The Hermes-style "View processing" body: the agent's narration and hidden
 * reasoning flow inline as prose, while runs of consecutive tool calls
 * collapse into a single group under a human summary ("Read 2 files"), each
 * step a sentence + a type icon, ending in a single "Done" check. Shared by
 * the process-source panel and (eventually) the inline rail so main-agent and
 * sub-agent activity render through one path.
 *
 * Falls back to a single tool group when no ordered transcript is present
 * (legacy snapshot), so older turns still show their steps.
 */
export function ProcessingTranscriptView({
  transcript,
  entries,
}: {
  transcript: ProcessingTranscriptItem[];
  entries: ToolTimelineEntry[];
}) {
  const blocks = buildProcessingBlocks(transcript, entries);
  if (blocks.length === 0) return null;

  return (
    <div className="space-y-2.5" data-testid="processing-transcript">
      {blocks.map(block => {
        if (block.kind === 'narration') {
          return (
            <p
              key={block.key}
              data-testid="processing-narration"
              className="text-[13px] leading-relaxed break-words whitespace-pre-wrap text-content-secondary">
              {block.text}
            </p>
          );
        }
        if (block.kind === 'thinking') {
          return <ThinkingBlock key={block.key} text={block.text} />;
        }
        return <ToolGroupBlock key={block.key} summary={block.summary} entries={block.entries} />;
      })}
    </div>
  );
}

/** The agent's hidden reasoning, rendered as a quiet collapsible block. */
function ThinkingBlock({ text }: { text: string }) {
  const { t } = useT();
  const clean = stripToolCallEnvelopes(text).trim();
  if (!clean) return null;
  return (
    <details
      data-testid="processing-thinking"
      className="group/think rounded-lg bg-surface-muted px-3 py-2">
      <summary className="flex cursor-pointer list-none items-center gap-1.5 select-none marker:hidden">
        <span aria-hidden className="text-[10px] leading-none">
          💭
        </span>
        <span className="text-[11px] font-semibold tracking-wide text-content-muted uppercase">
          {t('conversations.subagent.thinking')}
        </span>
        <span className="text-[9px] text-content-faint transition-transform group-open/think:rotate-90 dark:text-neutral-600">
          ▶
        </span>
      </summary>
      <p className="mt-1 text-[12px] leading-relaxed break-words whitespace-pre-wrap text-content-secondary">
        {clean}
      </p>
    </details>
  );
}

/** A collapsible group of consecutive tool rows under a human summary. */
function ToolGroupBlock({ summary, entries }: { summary: string; entries: ToolTimelineEntry[] }) {
  const { t } = useT();
  const allSettled = entries.every(e => e.status !== 'running');
  const anyError = entries.some(e => e.status === 'error');
  return (
    <details open className="group/group" data-testid="processing-tool-group">
      <summary className="flex cursor-pointer list-none items-center gap-1.5 select-none marker:hidden">
        <span className="text-[12px] font-medium text-content-secondary">{summary}</span>
        <span className="text-[9px] text-content-faint transition-transform group-open/group:rotate-90 dark:text-neutral-600">
          ▶
        </span>
      </summary>
      <ul className="mt-1 ml-1 space-y-1 border-l border-line pl-3">
        {entries.map(entry => (
          <ToolRow key={entry.id} entry={entry} />
        ))}
        {allSettled ? (
          <li className="flex items-center gap-1.5 pt-0.5">
            <StatusGlyph status={anyError ? 'error' : 'success'} />
            <span className="text-[11px] text-content-faint">
              {t('conversations.agentTaskInsights.done')}
            </span>
          </li>
        ) : null}
      </ul>
    </details>
  );
}

/**
 * The failure classes the UI has localized copy for (#4254 / #4459), keyed by the
 * camelCase form of the wire's PascalCase `class`. Any class not in this set
 * falls back to the English `causePlain` / `nextAction` carried on the payload.
 */
const LOCALIZED_FAILURE_CLASSES: ReadonlySet<string> = new Set([
  'missingPermission',
  'missingApp',
  'serviceUnavailable',
  'badCredentials',
  'blockedByPolicy',
  'modelConnection',
  'timeout',
  'denied',
  'approvalExpired',
  'unknown',
]);

/** Lowercase the first character: `MissingPermission` → `missingPermission`. */
function toCamelClass(cls: string): string {
  return cls.length > 0 ? cls[0].toLowerCase() + cls.slice(1) : cls;
}

/** One tool step: type icon + human sentence + contextual detail chip. */
function ToolRow({ entry }: { entry: ToolTimelineEntry }) {
  const { title, detail } = formatTimelineEntry(entry);
  return (
    <li className="flex items-start gap-1.5" data-testid="processing-tool-row">
      <span className="mt-0.5 shrink-0 text-content-faint">
        <CategoryIcon category={categorizeTool(entry.name)} />
      </span>
      <span className="min-w-0 text-[12px] text-content-secondary">
        {title}
        {detail ? (
          <span className="ml-1 rounded bg-surface-subtle px-1 py-px font-mono text-[10px] text-content-muted">
            {detail}
          </span>
        ) : null}
        {entry.status === 'error' && entry.failure ? (
          <ToolFailureLines failure={entry.failure} />
        ) : null}
      </span>
    </li>
  );
}

/**
 * The "why + what to do next" pair rendered under a failed tool row (#4254).
 * Copy is resolved by failure class from i18n, falling back to the English
 * `causePlain` / `nextAction` carried on the wire when the class is one the UI
 * hasn't localized. The failure line is coral to match the error glyph.
 */
function ToolFailureLines({ failure }: { failure: ToolFailureExplanation }) {
  const { t } = useT();
  const camel = toCamelClass(failure.class);
  const known = LOCALIZED_FAILURE_CLASSES.has(camel);
  const cause = known
    ? t(`conversations.toolFailure.${camel}.cause`, failure.causePlain)
    : failure.causePlain;
  const next = known
    ? t(`conversations.toolFailure.${camel}.next`, failure.nextAction)
    : failure.nextAction;
  return (
    <span
      data-testid="processing-tool-failure"
      className="mt-1 flex flex-col gap-0.5 text-[11px] leading-snug">
      <span className="text-coral-600 dark:text-coral-300">
        <span className="font-semibold">{t('conversations.toolFailure.whyLabel')}:</span> {cause}
      </span>
      <span className="text-content-muted">
        <span className="font-semibold">{t('conversations.toolFailure.nextLabel')}:</span> {next}
      </span>
    </span>
  );
}

/** Compact terminal status glyph for the group's "Done" line. */
function StatusGlyph({ status }: { status: ToolTimelineEntryStatus }) {
  if (status === 'error') {
    return <span className="text-[11px] text-coral-600 dark:text-coral-300">✕</span>;
  }
  return <span className="text-[11px] text-sage-600 dark:text-sage-300">✓</span>;
}

/** Minimal monochrome glyph per tool category (inherits `currentColor`). */
function CategoryIcon({ category }: { category: ToolCategory }) {
  const common = { width: 12, height: 12, viewBox: '0 0 12 12', 'aria-hidden': true } as const;
  switch (category) {
    case 'search':
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth={1.2}>
          <circle cx="5" cy="5" r="3.2" />
          <path d="M7.4 7.4 10.5 10.5" strokeLinecap="round" />
        </svg>
      );
    case 'run':
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth={1.2}>
          <rect x="1" y="1.5" width="10" height="9" rx="1.5" />
          <path d="M3 4.5 4.8 6 3 7.5M6 7.5h3" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      );
    case 'fetch':
    case 'browse':
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth={1}>
          <circle cx="6" cy="6" r="5" />
          <path d="M1 6h10M6 1c1.8 1.4 1.8 8.6 0 10M6 1c-1.8 1.4-1.8 8.6 0 10" />
        </svg>
      );
    case 'write':
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth={1.1}>
          <path d="M2.5 1.5h4L9.5 4.5V10.5H2.5z" strokeLinejoin="round" />
          <path d="M6.2 1.5V4.5H9.3M4.2 6.6 7.4 6.6M4.2 8.2 7.4 8.2" strokeLinecap="round" />
        </svg>
      );
    case 'read':
    default:
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth={1.1}>
          <path d="M2.5 1.5h4L9.5 4.5V10.5H2.5z" strokeLinejoin="round" />
          <path d="M6.2 1.5V4.5H9.3" strokeLinecap="round" />
        </svg>
      );
  }
}
