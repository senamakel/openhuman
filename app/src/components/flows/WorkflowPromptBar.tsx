/**
 * WorkflowPromptBar — the prompt-first "Copilot" authoring surface at the top
 * of the Flows page (and its empty-state hero). The user describes a workflow
 * in natural language, then chooses how to start:
 *
 *   • "Start building" (primary) — creates the blank flow and opens the canvas
 *     copilot CHAT-FIRST: the graph pane stays hidden and the copilot fills the
 *     surface until the build produces nodes ("graph appears later"). This is
 *     the conversational path — the user talks to the copilot instead of being
 *     dropped straight onto a raw graph.
 *   • "Build" (secondary) — the classic path: creates the blank flow and opens
 *     it directly on the graph canvas, copilot already building alongside.
 *
 * Both create the flow via `flows_create` (named from the description) and
 * navigate to `/flows/:id` with a `copilotBuild` seed in `location.state`; the
 * only difference is the seed's `chatFirst` flag, which the canvas reads to
 * pick the layout. The copilot's proposal keeps the usual gates: the agent only
 * PROPOSES; the user Accepts the diff and the canvas's explicit Save persists.
 */
import createDebug from 'debug';
import { useCallback, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { createBlankWorkflowGraph, deriveWorkflowName } from '../../lib/flows/newFlow';
import { useT } from '../../lib/i18n/I18nContext';
import { createFlow } from '../../services/api/flowsApi';
import Button from '../ui/Button';

const log = createDebug('app:flows:prompt-bar');

interface Props {
  /** Compact (list header) vs. hero (empty-state) presentation. */
  variant?: 'compact' | 'hero';
  /** Optional autofocus for the empty-state hero. */
  autoFocus?: boolean;
}

function SparkleIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M12 2l1.9 5.1L19 9l-5.1 1.9L12 16l-1.9-5.1L5 9l5.1-1.9L12 2z" />
      <path d="M19 14l.9 2.4L22 17l-2.1.6L19 20l-.9-2.4L16 17l2.1-.6L19 14z" opacity="0.7" />
    </svg>
  );
}

function PlayIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path d="M7 5l11 7-11 7V5z" fill="currentColor" />
    </svg>
  );
}

function ArrowUpIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true">
      <path d="M12 19V5M5 12l7-7 7 7" />
    </svg>
  );
}

export default function WorkflowPromptBar({ variant = 'compact', autoFocus = false }: Props) {
  const { t } = useT();
  const navigate = useNavigate();
  const [text, setText] = useState('');
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Create the blank flow (named from the description) and open its canvas.
  // `chatFirst` routes to the conversational copilot open ("Start building");
  // omitting it opens the classic graph canvas ("Build").
  const create = useCallback(
    async (chatFirst: boolean) => {
      const trimmed = text.trim();
      if (!trimmed || creating) return;
      setCreating(true);
      setError(null);
      const name = deriveWorkflowName(trimmed, t('flows.page.newWorkflow'));
      log('submit: creating flow name=%s chatFirst=%s', name, chatFirst);
      try {
        // Safe default: prompt-authored flows require approval so outbound
        // Slack/Gmail/HTTP/code nodes cannot fire unattended. Omitting this
        // arg would fall back to the server default of `false`.
        const flow = await createFlow(
          name,
          createBlankWorkflowGraph(name, t('flows.nodeKind.trigger')),
          true
        );
        log('submit: created id=%s — opening canvas (chatFirst=%s)', flow.id, chatFirst);
        navigate(`/flows/${flow.id}`, {
          state: {
            copilotBuild: chatFirst
              ? { description: trimmed, chatFirst: true }
              : { description: trimmed },
          },
        });
      } catch (err) {
        log('submit: create failed err=%o', err);
        setError(t('flows.promptBar.error'));
        setCreating(false);
      }
    },
    [text, creating, navigate, t]
  );

  const onKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      // Enter starts building (the primary, chat-first action); Shift+Enter
      // inserts a newline.
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        void create(true);
      }
    },
    [create]
  );

  const isHero = variant === 'hero';
  const disabled = creating || text.trim().length === 0;

  return (
    <div data-testid="workflow-prompt-bar">
      <section className="rounded-2xl border border-primary-300 bg-surface shadow-soft transition-colors focus-within:border-primary-500 dark:border-primary-700/60 dark:focus-within:border-primary-500">
        {/* Header: sparkle + Copilot wordmark, divided from the composer. */}
        <div className="flex items-center gap-2 border-b border-line px-4 py-2.5">
          <SparkleIcon className="h-4 w-4 text-primary-500" />
          <span className="text-sm font-semibold text-content">
            {t('flows.promptBar.copilotTitle')}
          </span>
        </div>

        <label htmlFor="workflow-prompt-input" className="sr-only">
          {t('flows.promptBar.label')}
        </label>
        <textarea
          id="workflow-prompt-input"
          data-testid="workflow-prompt-input"
          value={text}
          onChange={e => setText(e.target.value)}
          onKeyDown={onKeyDown}
          rows={isHero ? 3 : 2}
          autoFocus={autoFocus}
          disabled={creating}
          placeholder={t('flows.promptBar.placeholder')}
          className="w-full resize-none bg-transparent px-4 pt-3 text-sm text-content placeholder:text-content-faint focus:outline-none disabled:opacity-60"
        />

        {/* Footer: secondary "Build" (graph) + primary "Start building" (chat). */}
        <div className="flex items-center justify-end gap-3 px-4 pb-3 pt-2">
          <button
            type="button"
            data-testid="workflow-prompt-build"
            disabled={disabled}
            onClick={() => void create(false)}
            className="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-xs font-semibold text-primary-600 transition-colors hover:text-primary-700 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary-400 disabled:pointer-events-none disabled:opacity-40 dark:text-primary-300 dark:hover:text-primary-200">
            <PlayIcon className="h-3.5 w-3.5" />
            {t('flows.promptBar.submit')}
          </button>
          <Button
            type="button"
            variant="primary"
            size="sm"
            data-testid="workflow-prompt-submit"
            disabled={disabled}
            onClick={() => void create(true)}>
            <span className="inline-flex items-center gap-1.5">
              {creating ? t('flows.promptBar.thinking') : t('flows.promptBar.startBuilding')}
              {!creating && <ArrowUpIcon className="h-3.5 w-3.5" />}
            </span>
          </Button>
        </div>

        {error && (
          <p className="px-4 pb-3 text-xs text-coral" data-testid="workflow-prompt-error">
            {error}
          </p>
        )}
      </section>

      {/* AI disclaimer, mirroring the mockup — sits below the card. */}
      <p className="mt-2 text-center text-xs text-content-faint">
        {t('flows.promptBar.disclaimer')}
      </p>
    </div>
  );
}
