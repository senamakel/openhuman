import type {
  ThreadAssistantMessagePart,
  ThreadMessageLike,
  ThreadUserMessagePart,
} from '@assistant-ui/react';

import { parseMessageImages } from '../lib/attachments';
import { unwrapToolCallEnvelope } from '../lib/chat/toolCallEnvelope';
import type {
  ProcessingTranscriptItem,
  StreamingAssistantState,
  ToolTimelineEntry,
} from '../store/chatRuntimeSlice';
import type { ThreadMessage } from '../types/thread';

/**
 * Redux -> assistant-ui message mapping.
 *
 * assistant-ui is adopted as a *runtime* (semantics + API), never as a store:
 * `chatRuntimeSlice` and `threadSlice` remain the single source of truth for
 * messages, streaming, tool state, queueing and persistence. Everything here is
 * a pure, read-only projection of that state onto the shape the runtime wants.
 * Nothing in this module writes.
 *
 * The one property that matters for performance is stated as a test, not a
 * comment: converting the transcript while a token streams must not re-convert
 * the settled messages above the live tail. `ChatThreadView.renderPerf.test.tsx`
 * pins the equivalent property for the render tree; `assistantUiMessages.test.ts`
 * pins it for this projection.
 */

type ConversionCacheEntry = {
  timeline: readonly ToolTimelineEntry[];
  transcript: readonly ProcessingTranscriptItem[];
  converted: ThreadMessageLike;
};

/**
 * Cache keyed on the source message and its persisted process arrays. Socket
 * tokens only replace the live tail, so a settled message converts exactly
 * once while its transcript/timeline identities remain stable.
 */
const conversionCache = new WeakMap<ThreadMessage, ConversionCacheEntry>();

const EMPTY_TIMELINE: readonly ToolTimelineEntry[] = [];
const EMPTY_TRANSCRIPT: readonly ProcessingTranscriptItem[] = [];

/** Synthetic id for the live streaming tail. Stable so React reconciles it. */
export const STREAMING_TAIL_ID = '__openhuman_streaming_tail__';

/**
 * Convert one persisted message.
 *
 * Agent content is passed through `unwrapToolCallEnvelope` for the same reason
 * the transcript renderer does it: a `{content, tool_calls}` provider envelope
 * must never reach a rendered surface as raw JSON. Tool *activity* is not
 * projected as assistant-ui tool-call parts — it lives in the far richer
 * `toolTimelineByThread` projection that `ToolTimelineBlock` renders, and
 * duplicating it here would paint every tool twice.
 */
function jsonObject(value: unknown): Record<string, never> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
  try {
    return JSON.parse(JSON.stringify(value)) as Record<string, never>;
  } catch {
    return {};
  }
}

function toolArgs(entry: ToolTimelineEntry): Record<string, never> {
  if (!entry.argsBuffer) return {};
  try {
    return jsonObject(JSON.parse(entry.argsBuffer));
  } catch {
    return { raw: entry.argsBuffer } as never;
  }
}

function toolPart(entry: ToolTimelineEntry): ThreadAssistantMessagePart {
  const running = entry.status === 'running' || entry.status === 'awaiting_user';
  const isSubagent = entry.name.startsWith('subagent:') || entry.subagent !== undefined;
  const args = isSubagent
    ? jsonObject({
        subagent_type: entry.subagent?.agentId ?? entry.name.replace(/^subagent:/, ''),
        description: entry.detail,
        ...(running ? { progress: entry.subagent } : {}),
      })
    : toolArgs(entry);

  return {
    type: 'tool-call',
    toolCallId: entry.id,
    toolName: isSubagent ? 'task' : entry.name,
    args,
    argsText: JSON.stringify(args, null, 2),
    ...(!running
      ? {
          result: isSubagent
            ? (entry.subagent ?? { status: entry.status })
            : (entry.result ?? { status: entry.status, failure: entry.failure }),
        }
      : {}),
  };
}

/**
 * Project one assistant message into assistant-ui parts.
 *
 * **Every tool part must have a distinct `toolCallId`.** assistant-ui keys them
 * as `toolCallId-${id}` and *throws* on a repeat ("Duplicate key … in
 * useResources"), which takes the whole thread render down rather than dropping
 * a row — so this is a hard invariant, not a tidiness rule, and it is enforced
 * here at the boundary as well as at each producer. `emittedToolIds` guards
 * both passes below; the sources upstream (the live Redux slice and the derived
 * transcript mapper) also mint unique ids, but threads persisted before those
 * fixes still carry colliding ones.
 */
function assistantParts(
  text: string,
  timeline: readonly ToolTimelineEntry[],
  transcript: readonly ProcessingTranscriptItem[]
): ThreadAssistantMessagePart[] {
  const parts: ThreadAssistantMessagePart[] = [];
  const timelineById = new Map(timeline.map(entry => [entry.id, entry]));
  const emittedToolIds = new Set<string>();

  for (const item of transcript) {
    if (item.kind === 'thinking') {
      if (item.text.trim().length > 0) parts.push({ type: 'reasoning', text: item.text });
      continue;
    }
    if (item.kind === 'toolCall') {
      const entry = timelineById.get(item.callId);
      // Guarded here too, not only in the timeline pass below: two transcript
      // pointers can name the same `callId` (a provider that emits tool calls
      // without ids writes the empty string for all of them), and both resolve
      // to the same timeline row.
      if (entry && !emittedToolIds.has(entry.id)) {
        emittedToolIds.add(entry.id);
        parts.push(toolPart(entry));
      }
    }
    // Narration is process commentary, not the final assistant answer. The
    // legacy pane keeps it in the processing view; projecting it as ordinary
    // text here would duplicate prose around the persisted final response.
  }

  for (const entry of [...timeline].sort((a, b) => a.seq - b.seq)) {
    if (emittedToolIds.has(entry.id)) continue;
    emittedToolIds.add(entry.id);
    parts.push(toolPart(entry));
  }
  if (text.length > 0) parts.push({ type: 'text', text });
  return parts;
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : [];
}

function mimeTypeFromDataUri(dataUri: string): string {
  return dataUri.match(/^data:([^;,]+)/i)?.[1] ?? 'application/octet-stream';
}

function userParts(msg: ThreadMessage): ThreadUserMessagePart[] {
  const parsed = parseMessageImages(msg.content ?? '');
  const metadata = msg.extraMetadata ?? {};
  const kinds = stringArray(metadata.attachmentKinds);
  const names = stringArray(metadata.attachmentNames);
  const posters = stringArray(metadata.attachmentPosters);
  const metadataUris = stringArray(metadata.attachmentDataUris);
  const dataUris = metadataUris.length > 0 ? metadataUris : parsed.dataUris;
  const parts: ThreadUserMessagePart[] = [];

  if (parsed.text.length > 0) parts.push({ type: 'text', text: parsed.text });

  if (kinds.length === 0) {
    for (const [index, image] of dataUris.entries()) {
      parts.push({ type: 'image', image, filename: names[index] });
    }
    return parts;
  }

  for (const [index, kind] of kinds.entries()) {
    const filename = names[index];
    if (kind === 'image') {
      const image = dataUris[index];
      if (image) parts.push({ type: 'image', image, filename });
      continue;
    }
    if (kind === 'video') {
      const image = posters[index];
      if (image) parts.push({ type: 'image', image, filename });
      else parts.push({ type: 'file', filename, data: '', mimeType: 'video/mp4' });
      continue;
    }
    const data = dataUris[index] ?? '';
    parts.push({ type: 'file', filename, data, mimeType: mimeTypeFromDataUri(data) });
  }
  return parts;
}

export function toThreadMessageLike(
  msg: ThreadMessage,
  timeline: readonly ToolTimelineEntry[] = EMPTY_TIMELINE,
  transcript: readonly ProcessingTranscriptItem[] = EMPTY_TRANSCRIPT
): ThreadMessageLike {
  const cached = conversionCache.get(msg);
  if (cached?.timeline === timeline && cached.transcript === transcript) return cached.converted;

  const text =
    msg.sender === 'agent' ? unwrapToolCallEnvelope(msg.content ?? '').text : (msg.content ?? '');

  const converted: ThreadMessageLike = {
    id: msg.id,
    role: msg.sender === 'agent' ? 'assistant' : 'user',
    content: msg.sender === 'agent' ? assistantParts(text, timeline, transcript) : userParts(msg),
    createdAt: new Date(msg.createdAt),
    ...(msg.sender === 'agent' && msg.extraMetadata?.stopped === true
      ? { status: { type: 'incomplete' as const, reason: 'cancelled' as const } }
      : {}),
    metadata: { custom: { extraMetadata: msg.extraMetadata ?? {}, sourceType: msg.type } },
  };

  conversionCache.set(msg, { timeline, transcript, converted });
  return converted;
}

/**
 * The live tail as a running assistant message.
 *
 * The tail is deliberately NOT part of `thread.messagesByThreadId` — Redux keeps
 * the settled transcript and the in-flight preview in separate slices, which is
 * exactly what keeps settled message identities stable while tokens land. Here
 * that separation is re-joined for the runtime's benefit: one fresh object per
 * token, and only that one object is ever re-converted.
 */
export function streamingTailMessage(
  streaming: StreamingAssistantState | null,
  timeline: readonly ToolTimelineEntry[] = EMPTY_TIMELINE,
  transcript: readonly ProcessingTranscriptItem[] = EMPTY_TRANSCRIPT
): ThreadMessageLike | null {
  if (!streaming && timeline.length === 0 && transcript.length === 0) return null;
  const text = streaming?.content ?? '';
  const parts = assistantParts(text, timeline, transcript);
  if (streaming?.thinking.trim()) {
    const hasTranscriptThinking = transcript.some(item => item.kind === 'thinking');
    if (!hasTranscriptThinking) parts.unshift({ type: 'reasoning', text: streaming.thinking });
  }
  if (parts.length === 0) return null;
  return {
    id: STREAMING_TAIL_ID,
    role: 'assistant',
    content: parts,
    status: { type: 'running' },
    metadata: { custom: { requestId: streaming?.requestId, streaming: true } },
  };
}

export type AssistantUiProjection = {
  liveTimeline?: readonly ToolTimelineEntry[];
  liveTranscript?: readonly ProcessingTranscriptItem[];
  turnTimelines?: Readonly<Record<string, readonly ToolTimelineEntry[]>>;
  turnTranscripts?: Readonly<Record<string, readonly ProcessingTranscriptItem[]>>;
};

/**
 * The full thread as assistant-ui sees it: settled transcript, then the live
 * tail when one is in flight.
 *
 * Hidden messages are filtered the same way the transcript filters them, so the
 * runtime's view of the thread and the rendered view cannot disagree about what
 * the conversation contains.
 */
export function buildRuntimeMessages(
  messages: readonly ThreadMessage[],
  streaming: StreamingAssistantState | null,
  projection: AssistantUiProjection = {}
): ThreadMessageLike[] {
  const out: ThreadMessageLike[] = [];
  for (const msg of messages) {
    if (msg.extraMetadata?.hidden) continue;
    const requestId =
      msg.sender === 'agent' && typeof msg.extraMetadata?.requestId === 'string'
        ? msg.extraMetadata.requestId
        : undefined;
    out.push(
      toThreadMessageLike(
        msg,
        requestId ? (projection.turnTimelines?.[requestId] ?? EMPTY_TIMELINE) : EMPTY_TIMELINE,
        requestId ? (projection.turnTranscripts?.[requestId] ?? EMPTY_TRANSCRIPT) : EMPTY_TRANSCRIPT
      )
    );
  }
  const tail = streamingTailMessage(
    streaming,
    projection.liveTimeline ?? EMPTY_TIMELINE,
    projection.liveTranscript ?? EMPTY_TRANSCRIPT
  );
  if (tail) out.push(tail);
  return out;
}
