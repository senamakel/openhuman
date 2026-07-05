/**
 * WorkflowCopilotPanel (Phase 5c) — a side-panel chat bound to the
 * `workflow_builder` specialist, docked on the editable canvas. The user asks
 * for changes ("add a Slack notification on failure", "make the schedule
 * weekdays only"); each turn injects the CURRENT draft graph as context and the
 * agent returns a `revise_workflow` proposal (and can now discover + connect the
 * Composio apps a step needs). The panel renders the full conversation
 * transcript, surfaces each proposal's node-level diff, and hands Accept/Reject
 * up to the host, which applies it to the local draft overlay.
 *
 * Chat UI parity: the composer is the same {@link ChatComposer} the main chat
 * windows use (mic/attachments off here), and turns render as bubbles via the
 * shared {@link BubbleMarkdown}, so the copilot reads like a real chat rather
 * than a one-shot form.
 *
 * Invariant: the copilot only PROPOSES. Accept applies to the UNSAVED local
 * draft (no `flows_update`); persistence stays behind the canvas's own Save.
 */
import { useCallback, useEffect, useRef, useState } from 'react';

import { useWorkflowBuilderChat } from '../../hooks/useWorkflowBuilderChat';
import { diffGraphs } from '../../lib/flows/graphDiff';
import type { WorkflowGraph } from '../../lib/flows/types';
import {
  buildRepairPrompt,
  buildRevisePrompt,
  type RepairPromptContext,
} from '../../lib/flows/workflowBuilderPrompt';
import { useT } from '../../lib/i18n/I18nContext';
import { BubbleMarkdown } from '../../pages/conversations/components/AgentMessageBubble';
import type { WorkflowProposal } from '../../store/chatRuntimeSlice';
import ChatComposer from '../chat/ChatComposer';
import Button from '../ui/Button';

interface Props {
  /** The current draft graph, injected as context for each revise turn. */
  graph: WorkflowGraph;
  /**
   * The saved flow's id (or `null`/absent for an unsaved draft), injected into
   * revise turns so the agent can `run_workflow` it to test — with confirmation.
   */
  flowId?: string | null;
  /**
   * Fires when the agent returns a fresh proposal, so the host can enter its
   * diff-preview overlay. The host computes/holds the preview; this panel only
   * reflects it.
   */
  onProposal: (proposal: WorkflowProposal) => void;
  /** Accept the pending proposal into the local draft (host commits it). */
  onAccept: (proposal: WorkflowProposal) => void;
  /** Reject the pending proposal (host reverts the overlay). */
  onReject: () => void;
  /** Close the panel. */
  onClose: () => void;
  /**
   * Optional repair seed (from a failed run's "Fix with agent") — auto-sends a
   * repair turn once on mount so the copilot opens already diagnosing.
   */
  repairSeed?: RepairPromptContext | null;
  /**
   * The workflow's persisted copilot thread id (from the per-flow cache), so
   * reopening the panel resumes the same conversation instead of starting fresh.
   */
  seedThreadId?: string | null;
  /** Reports the live thread id up so the host can persist it per workflow. */
  onThreadIdChange?: (threadId: string | null) => void;
}

export default function WorkflowCopilotPanel({
  graph,
  flowId = null,
  onProposal,
  onAccept,
  onReject,
  onClose,
  repairSeed = null,
  seedThreadId = null,
  onThreadIdChange,
}: Props) {
  const { t } = useT();
  const { threadId, sending, proposal, messages, error, send, clearProposal } =
    useWorkflowBuilderChat(seedThreadId);
  const [text, setText] = useState('');

  // Report the (lazily-created) thread id up so the host persists it per flow —
  // reopening the copilot then resumes this same conversation.
  useEffect(() => {
    onThreadIdChange?.(threadId);
  }, [threadId, onThreadIdChange]);

  // ChatComposer plumbing (mic/attachments are off, so most refs are inert).
  const textInputRef = useRef<HTMLTextAreaElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const isComposingTextRef = useRef(false);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  // Surface each NEW proposal to the host exactly once (enter preview overlay).
  const lastSurfacedRef = useRef<WorkflowProposal | null>(null);
  useEffect(() => {
    if (proposal && proposal !== lastSurfacedRef.current) {
      lastSurfacedRef.current = proposal;
      onProposal(proposal);
    }
  }, [proposal, onProposal]);

  // Auto-send the repair turn once when opened from a failed run.
  const repairSentRef = useRef(false);
  useEffect(() => {
    if (!repairSeed || repairSentRef.current) return;
    repairSentRef.current = true;
    void send({
      displayText: t('flows.copilot.repairDisplay'),
      prompt: buildRepairPrompt(repairSeed),
    });
  }, [repairSeed, send, t]);

  // Keep the transcript pinned to the newest message / thinking indicator.
  // `scrollTo` is optional-chained: jsdom (tests) doesn't implement it.
  useEffect(() => {
    scrollRef.current?.scrollTo?.({ top: scrollRef.current.scrollHeight });
  }, [messages, sending, proposal]);

  const submit = useCallback(
    async (raw?: string) => {
      const trimmed = (raw ?? text).trim();
      if (!trimmed || sending) return;
      setText('');
      await send({ displayText: trimmed, prompt: buildRevisePrompt(trimmed, graph, flowId) });
    },
    [text, sending, send, graph, flowId]
  );

  const handleInputKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key === 'Enter' && !event.shiftKey && !isComposingTextRef.current) {
        event.preventDefault();
        void submit();
      }
    },
    [submit]
  );

  const noopAttach = useCallback(async () => {}, []);
  const noop = useCallback(() => {}, []);

  const accept = useCallback(() => {
    if (!proposal) return;
    onAccept(proposal);
    clearProposal();
    lastSurfacedRef.current = null;
  }, [proposal, onAccept, clearProposal]);

  const reject = useCallback(() => {
    onReject();
    clearProposal();
    lastSurfacedRef.current = null;
  }, [onReject, clearProposal]);

  const diff = proposal ? diffGraphs(graph, proposal.graph as WorkflowGraph) : null;
  const isEmpty = messages.length === 0 && !proposal && !sending && !error;

  return (
    <aside
      data-testid="workflow-copilot-panel"
      className="flex h-full w-full max-w-sm flex-col border-l border-line bg-surface">
      <header className="flex items-start gap-2 border-b border-line px-3 py-2.5">
        <div className="min-w-0 flex-1">
          <p className="text-sm font-semibold text-content">{t('flows.copilot.title')}</p>
          <p className="text-[11px] text-content-muted">{t('flows.copilot.subtitle')}</p>
        </div>
        <button
          type="button"
          data-testid="workflow-copilot-close"
          aria-label={t('flows.copilot.close')}
          onClick={onClose}
          className="shrink-0 rounded-full p-1.5 text-content-faint hover:bg-surface-hover hover:text-content-secondary">
          ✕
        </button>
      </header>

      <div
        ref={scrollRef}
        className="flex-1 space-y-3 overflow-y-auto px-3 py-3"
        data-testid="workflow-copilot-transcript">
        {isEmpty && (
          <p className="text-xs text-content-muted" data-testid="workflow-copilot-empty">
            {t('flows.copilot.emptyState')}
          </p>
        )}

        {/* Conversation transcript: user turns right-aligned, agent turns left. */}
        {messages.map(message =>
          message.sender === 'user' ? (
            <div key={message.id} className="flex justify-end" data-testid="workflow-copilot-user">
              <div className="max-w-[85%] rounded-2xl bg-primary-500 px-3 py-1.5 text-sm text-content-inverted">
                {message.content}
              </div>
            </div>
          ) : (
            <div
              key={message.id}
              className="max-w-[92%] rounded-2xl bg-surface-subtle px-3 py-1.5"
              data-testid="workflow-copilot-agent">
              <BubbleMarkdown content={message.content} />
            </div>
          )
        )}

        {sending && (
          <p className="text-xs text-content-muted" data-testid="workflow-copilot-thinking">
            {t('flows.copilot.thinking')}
          </p>
        )}

        {error && (
          <p className="text-xs text-coral" data-testid="workflow-copilot-error">
            {error === 'offline' ? t('flows.copilot.offline') : t('flows.copilot.error')}
          </p>
        )}

        {proposal && diff && (
          <div
            data-testid="workflow-copilot-proposal"
            className="rounded-xl border border-ocean-300 bg-surface p-3 dark:border-ocean-700">
            <p className="text-xs font-semibold text-ocean-900 dark:text-ocean-100">
              {proposal.name || t('flows.copilot.proposalTitle')}
            </p>
            <p className="mt-1 text-[11px] text-content-muted">{t('flows.copilot.previewHint')}</p>

            <div className="mt-2 flex flex-wrap gap-1.5 text-[11px]">
              {diff.addedNodeIds.size > 0 && (
                <span
                  data-testid="workflow-copilot-added"
                  className="rounded-full bg-sage-100 px-2 py-0.5 font-medium text-sage-700 dark:bg-sage-500/15 dark:text-sage-300">
                  {t('flows.copilot.added').replace('{count}', String(diff.addedNodeIds.size))}
                </span>
              )}
              {diff.removedNodeIds.size > 0 && (
                <span
                  data-testid="workflow-copilot-removed"
                  className="rounded-full bg-coral-100 px-2 py-0.5 font-medium text-coral-700 dark:bg-coral-500/15 dark:text-coral-300">
                  {t('flows.copilot.removed').replace('{count}', String(diff.removedNodeIds.size))}
                </span>
              )}
              {!diff.hasChanges && (
                <span className="text-content-faint">{t('flows.copilot.noChanges')}</span>
              )}
            </div>

            <div className="mt-3 flex items-center gap-2">
              <Button
                type="button"
                variant="primary"
                size="sm"
                data-testid="workflow-copilot-accept"
                onClick={accept}>
                {t('flows.copilot.accept')}
              </Button>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                data-testid="workflow-copilot-reject"
                onClick={reject}>
                {t('flows.copilot.reject')}
              </Button>
            </div>
          </div>
        )}
      </div>

      <div className="border-t border-line px-3 py-2.5">
        <ChatComposer
          inputValue={text}
          setInputValue={setText}
          onSend={submit}
          textInputRef={textInputRef}
          fileInputRef={fileInputRef}
          composerInteractionBlocked={sending}
          isSending={sending}
          attachments={[]}
          onAttachFiles={noopAttach}
          onRemoveAttachment={noop}
          attachError={null}
          onSwitchToMicCloud={noop}
          handleInputKeyDown={handleInputKeyDown}
          inlineCompletionSuffix=""
          isComposingTextRef={isComposingTextRef}
          maxAttachments={0}
          allowedMimeTypes={[]}
          attachmentsEnabled={false}
          micEnabled={false}
          placeholder={t('flows.copilot.placeholder')}
        />
      </div>
    </aside>
  );
}
