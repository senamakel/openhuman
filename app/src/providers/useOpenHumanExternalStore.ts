import type { AppendMessage } from '@assistant-ui/react';
import { useCallback, useMemo } from 'react';

import { useAppSelector } from '../store/hooks';
import type { ThreadMessage } from '../types/thread';
import { buildRuntimeMessages } from './assistantUiMessages';
import { getChatSurface } from './chatSurfaceHandlers';

const EMPTY_MESSAGES: ThreadMessage[] = [];
const EMPTY_TIMELINE: never[] = [];
const EMPTY_TRANSCRIPT: never[] = [];
const EMPTY_TURN_MAP = {};

/** Flatten an assistant-ui append payload down to the plain text our core takes. */
function appendMessageText(message: AppendMessage): string {
  return message.content
    .map(part => (part.type === 'text' ? part.text : ''))
    .join('')
    .trim();
}

/**
 * Build the `ExternalStoreAdapter` that backs `useExternalStoreRuntime`.
 *
 * Read side: a pure projection of Redux. Write side: forwarded to the surface
 * that owns the thread (see `chatSurfaceHandlers`). The runtime therefore gets
 * a faithful, live view of the conversation and a working action API without
 * holding any state of its own.
 */
export function useOpenHumanExternalStore(threadId: string | null) {
  const messages = useAppSelector(state =>
    threadId ? (state.thread.messagesByThreadId[threadId] ?? EMPTY_MESSAGES) : EMPTY_MESSAGES
  );
  const streaming = useAppSelector(state =>
    threadId ? (state.chatRuntime.streamingAssistantByThread?.[threadId] ?? null) : null
  );
  const lifecycle = useAppSelector(state =>
    threadId ? (state.chatRuntime.inferenceTurnLifecycleByThread?.[threadId] ?? null) : null
  );
  const isLoading = useAppSelector(state => Boolean(threadId && state.thread.isLoadingMessages));
  const liveTimeline = useAppSelector(state =>
    threadId
      ? (state.chatRuntime.toolTimelineByThread?.[threadId] ?? EMPTY_TIMELINE)
      : EMPTY_TIMELINE
  );
  const liveTranscript = useAppSelector(state =>
    threadId
      ? (state.chatRuntime.processingByThread?.[threadId] ?? EMPTY_TRANSCRIPT)
      : EMPTY_TRANSCRIPT
  );
  const turnTimelines = useAppSelector(state =>
    threadId
      ? (state.chatRuntime.turnTimelinesByThread?.[threadId] ?? EMPTY_TURN_MAP)
      : EMPTY_TURN_MAP
  );
  const turnTranscripts = useAppSelector(state =>
    threadId
      ? (state.chatRuntime.turnTranscriptsByThread?.[threadId] ?? EMPTY_TURN_MAP)
      : EMPTY_TURN_MAP
  );

  // Recomputed only when the settled transcript or the live tail changes.
  // Settled messages are converted through an identity-keyed cache, so a token
  // landing on the tail re-converts exactly one message, never the transcript.
  const runtimeMessages = useMemo(
    () =>
      buildRuntimeMessages(messages, streaming, {
        liveTimeline,
        liveTranscript,
        turnTimelines,
        turnTranscripts,
      }),
    [messages, streaming, liveTimeline, liveTranscript, turnTimelines, turnTranscripts]
  );

  // `started` and `streaming` are both in-flight; the row is deleted on
  // completion, so a present lifecycle (other than the cold-boot `interrupted`
  // marker, which has no live driver) means a turn is running.
  const isRunning = lifecycle === 'started' || lifecycle === 'streaming';

  const onNew = useCallback(
    async (message: AppendMessage) => {
      const surface = getChatSurface(threadId);
      // Fail loudly. A silent no-op here would look like a dropped message.
      if (!surface) {
        throw new Error(`No chat surface registered for thread ${threadId ?? '(none)'}`);
      }
      const text = appendMessageText(message);
      if (text.length === 0) return;
      await surface.send(text);
    },
    [threadId]
  );

  const onCancel = useCallback(async () => {
    await getChatSurface(threadId)?.cancel?.();
  }, [threadId]);

  return useMemo(
    () => ({
      messages: runtimeMessages,
      isRunning,
      isLoading,
      // Already `ThreadMessageLike`; the runtime's converter is the identity.
      convertMessage: (m: (typeof runtimeMessages)[number]) => m,
      onNew,
      onCancel,
    }),
    [runtimeMessages, isRunning, isLoading, onNew, onCancel]
  );
}
