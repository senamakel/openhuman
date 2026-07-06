/**
 * useWorkflowBuilderChat — drives the Flows prompt bar and canvas copilot by
 * running the `workflow_builder` agent server-side. It owns a DEDICATED thread
 * (created lazily on first send) so an authoring conversation never collides
 * with the user's main chat, sends a STRUCTURED turn request to
 * `openhuman.flows_build` (which renders the brief and runs the agent), and
 * surfaces the returned `WorkflowProposal` on this thread.
 *
 * The builder is now a first-class backend agent (like the Flow Scout): the core
 * constructs the prompt and drives the agent to completion, then returns its
 * proposal + final text. This hook no longer crafts delegate prompt strings or
 * routes through the chat orchestrator — it appends the user turn + the agent's
 * reply to the same `messagesByThreadId` transcript and sets the proposal in the
 * same `pendingWorkflowProposalsByThread` slice the streamed path used, so the
 * UI renders unchanged.
 *
 * Invariant: `create`/`revise`/`repair` never persist; only a `build` turn (with
 * a real flow id) may save onto an existing flow. Nothing here enables a flow.
 */
import createDebug from 'debug';
import { useCallback, useMemo, useState } from 'react';

import { buildWorkflow, type BuilderTurnRequest } from '../services/api/flowsApi';
import {
  clearWorkflowProposalForThread,
  setWorkflowProposalForThread,
  type WorkflowProposal,
} from '../store/chatRuntimeSlice';
import { useAppDispatch, useAppSelector } from '../store/hooks';
import { selectSocketStatus } from '../store/socketSelectors';
import { addMessageLocal, createNewThread } from '../store/threadSlice';
import type { ThreadMessage } from '../types/thread';

const log = createDebug('app:flows:builder-chat');

/** A single builder turn: what the user sees vs. the structured turn request. */
export interface WorkflowBuilderSendParams {
  /** Human-readable text shown as the user's message in the thread transcript. */
  displayText: string;
  /**
   * The structured builder-turn request. The core renders the agent's brief
   * from this and runs `workflow_builder` directly (via `openhuman.flows_build`)
   * — the frontend no longer crafts delegate prompt strings.
   */
  request: BuilderTurnRequest;
}

export interface UseWorkflowBuilderChat {
  /** The dedicated thread id, or `null` before the first send creates it. */
  threadId: string | null;
  /** True while a builder turn is in flight on this thread. */
  sending: boolean;
  /** The latest proposal the agent returned on this thread, or `null`. */
  proposal: WorkflowProposal | null;
  /**
   * The dedicated thread's transcript (user + agent turns) so a caller can
   * render the full conversation, not just the latest proposal. Empty until the
   * first send. Sourced from the same `messagesByThreadId` store the main chat
   * transcript reads.
   */
  messages: ThreadMessage[];
  /** Last send error (thread create / RPC failure), or `null`. */
  error: string | null;
  /** Send a builder turn, creating the dedicated thread on first use. */
  send: (params: WorkflowBuilderSendParams) => Promise<void>;
  /** Clear the current proposal (e.g. after Accept/Reject) without persisting. */
  clearProposal: () => void;
}

const EMPTY_MESSAGES: ThreadMessage[] = [];

/**
 * @param seedThreadId Optional existing thread to bind to instead of creating a
 *   fresh one — lets a caller reuse a thread across mounts (unused today; the
 *   prompt bar and copilot each start clean).
 */
export function useWorkflowBuilderChat(seedThreadId?: string | null): UseWorkflowBuilderChat {
  const dispatch = useAppDispatch();
  const socketStatus = useAppSelector(selectSocketStatus);
  const [threadId, setThreadId] = useState<string | null>(seedThreadId ?? null);
  const [localSending, setLocalSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const proposalsByThread = useAppSelector(
    state => state.chatRuntime.pendingWorkflowProposalsByThread
  );
  const messagesByThreadId = useAppSelector(state => state.thread.messagesByThreadId);

  const proposal = useMemo(
    () => (threadId ? (proposalsByThread[threadId] ?? null) : null),
    [threadId, proposalsByThread]
  );

  const messages = useMemo(
    () => (threadId ? (messagesByThreadId[threadId] ?? EMPTY_MESSAGES) : EMPTY_MESSAGES),
    [threadId, messagesByThreadId]
  );

  // The turn is a single request/response RPC (no streaming runtime), so
  // "sending" is simply whether that call is in flight.
  const sending = localSending;

  const send = useCallback(
    async ({ displayText, request }: WorkflowBuilderSendParams) => {
      if (localSending) {
        log('send: ignored — a turn is already dispatching');
        return;
      }
      if (socketStatus !== 'connected') {
        log('send: blocked — socket not connected (%s)', socketStatus);
        setError('offline');
        return;
      }
      setLocalSending(true);
      setError(null);
      let targetThreadId = threadId;
      try {
        if (!targetThreadId) {
          log('send: creating dedicated builder thread');
          const thread = await dispatch(createNewThread(['workflow-builder'])).unwrap();
          targetThreadId = thread.id;
          setThreadId(targetThreadId);
        }
        // A fresh turn supersedes any prior proposal on this thread.
        dispatch(clearWorkflowProposalForThread({ threadId: targetThreadId }));

        const userMessage: ThreadMessage = {
          id: `msg_${globalThis.crypto.randomUUID()}`,
          content: displayText,
          type: 'text',
          extraMetadata: {},
          sender: 'user',
          createdAt: new Date().toISOString(),
        };
        await dispatch(
          addMessageLocal({ threadId: targetThreadId, message: userMessage })
        ).unwrap();

        // Run the workflow_builder agent server-side and get its proposal +
        // final text back directly (no streaming). `openhuman.flows_build`
        // renders the brief and runs the agent — the frontend sends only the
        // structured request.
        log('send: running flows_build thread=%s mode=%s', targetThreadId, request.mode);
        const result = await buildWorkflow(request);

        // Render the agent's final text as its turn in the transcript.
        if (result.assistantText.trim().length > 0) {
          const agentMessage: ThreadMessage = {
            id: `msg_${globalThis.crypto.randomUUID()}`,
            content: result.assistantText,
            type: 'text',
            extraMetadata: {},
            sender: 'agent',
            createdAt: new Date().toISOString(),
          };
          await dispatch(
            addMessageLocal({ threadId: targetThreadId, message: agentMessage })
          ).unwrap();
        }

        // Surface the proposal via the same store slice the streamed path used,
        // so `WorkflowProposalCard` / the copilot preview render unchanged.
        if (result.proposal) {
          dispatch(
            setWorkflowProposalForThread({
              threadId: targetThreadId,
              proposal: result.proposal,
            })
          );
        } else if (result.error) {
          setError(result.error);
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        log('send: failed err=%o', err);
        setError(msg);
      } finally {
        setLocalSending(false);
      }
    },
    [dispatch, localSending, socketStatus, threadId]
  );

  const clearProposal = useCallback(() => {
    if (threadId) dispatch(clearWorkflowProposalForThread({ threadId }));
  }, [dispatch, threadId]);

  return { threadId, sending, proposal, messages, error, send, clearProposal };
}
