/**
 * useWorkflowBuilderChat — drives the Flows prompt bar and canvas copilot by
 * running the `workflow_builder` agent server-side. It owns a DEDICATED thread
 * (created lazily on first send) so an authoring conversation never collides
 * with the user's main chat, sends a STRUCTURED turn request to
 * `openhuman.flows_build` (which renders the brief and runs the agent), and
 * surfaces the returned `WorkflowProposal` on this thread.
 *
 * The builder is now a first-class backend agent (like the Flow Scout): the core
 * constructs the prompt and drives the agent to completion. Phase B streams that
 * turn onto the copilot's dedicated thread (text / thinking / tool events +
 * a terminal `chat_done`), so this hook passes its `threadId` into
 * `openhuman.flows_build` and lets the GLOBAL `ChatRuntimeProvider` own the
 * transcript: the provider appends the final assistant message on `chat_done`
 * and populates `streamingAssistantByThread` / `toolTimelineByThread` /
 * `pendingWorkflowProposalsByThread` for this thread as the turn runs. This hook
 * only appends the local USER turn (the web channel never persists user
 * messages) and reads the streamed state back out; the blocking
 * `{proposal, error}` return is a fallback for when streaming isn't wired
 * (CLI / tests / a missed socket event).
 *
 * Invariant: `create`/`revise`/`repair` never persist; only a `build` turn (with
 * a real flow id) may save onto an existing flow. Nothing here enables a flow.
 */
import createDebug from 'debug';
import { useCallback, useMemo, useState } from 'react';

import { type BuilderTurnRequest, buildWorkflow } from '../services/api/flowsApi';
import {
  clearWorkflowProposalForThread,
  setWorkflowProposalForThread,
  type ToolTimelineEntry,
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
  /**
   * The dedicated thread's live tool timeline (streamed by `ChatRuntimeProvider`
   * as the builder turn runs) — bound straight into the shared
   * `ToolTimelineBlock`. Empty when nothing has streamed on this thread.
   */
  toolTimeline: ToolTimelineEntry[];
  /**
   * The builder turn's in-flight assistant text (the shared streaming lane), for
   * `ToolTimelineBlock`'s `liveResponse`. Empty string once the turn settles —
   * the final answer then lives in `messages`.
   */
  liveResponse: string;
  /** Last send error (thread create / RPC failure), or `null`. */
  error: string | null;
  /** Send a builder turn, creating the dedicated thread on first use. */
  send: (params: WorkflowBuilderSendParams) => Promise<void>;
  /** Clear the current proposal (e.g. after Accept/Reject) without persisting. */
  clearProposal: () => void;
}

const EMPTY_MESSAGES: ThreadMessage[] = [];
const EMPTY_TIMELINE: ToolTimelineEntry[] = [];

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
  const toolTimelineByThread = useAppSelector(state => state.chatRuntime.toolTimelineByThread);
  const streamingAssistantByThread = useAppSelector(
    state => state.chatRuntime.streamingAssistantByThread
  );

  // Prefer the runtime's streamed proposal (populated on this thread by
  // `ChatRuntimeProvider` as the builder's `propose_workflow`/`revise_workflow`
  // tool result lands); the blocking `send` result is only a fallback that
  // writes into the same slice.
  const proposal = useMemo(
    () => (threadId ? (proposalsByThread[threadId] ?? null) : null),
    [threadId, proposalsByThread]
  );

  const messages = useMemo(
    () => (threadId ? (messagesByThreadId[threadId] ?? EMPTY_MESSAGES) : EMPTY_MESSAGES),
    [threadId, messagesByThreadId]
  );

  const toolTimeline = useMemo(
    () => (threadId ? (toolTimelineByThread[threadId] ?? EMPTY_TIMELINE) : EMPTY_TIMELINE),
    [threadId, toolTimelineByThread]
  );

  const liveResponse = useMemo(
    () => (threadId ? (streamingAssistantByThread[threadId]?.content ?? '') : ''),
    [threadId, streamingAssistantByThread]
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

        // Run the workflow_builder agent server-side, streaming its turn onto
        // this thread (Phase B): passing `targetThreadId` makes the core emit
        // text/thinking/tool events + a terminal `chat_done` keyed by it. The
        // GLOBAL `ChatRuntimeProvider` owns that transcript — it appends the
        // final assistant message on `chat_done` and fills the streaming/tool
        // slices as the turn runs — so this hook must NOT also append the agent
        // reply (doing so would double it). We still await the blocking result
        // for its `proposal`/`error` fallback.
        log('send: running flows_build thread=%s mode=%s', targetThreadId, request.mode);
        const result = await buildWorkflow(request, targetThreadId);

        // Surface the proposal via the same store slice the streamed path used,
        // so `WorkflowProposalCard` / the copilot preview render unchanged. This
        // is a fallback: when streaming is wired the runtime already populated
        // `pendingWorkflowProposalsByThread` from the tool result; re-writing the
        // same value here is idempotent and covers a missed socket event / CLI.
        if (result.proposal) {
          dispatch(
            setWorkflowProposalForThread({ threadId: targetThreadId, proposal: result.proposal })
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

  return {
    threadId,
    sending,
    proposal,
    messages,
    toolTimeline,
    liveResponse,
    error,
    send,
    clearProposal,
  };
}
