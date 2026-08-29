import { describe, expect, it, vi } from 'vitest';

import type { ToolTimelineEntry } from '../../store/chatRuntimeSlice';
import type { ThreadMessage } from '../../types/thread';
import {
  buildRuntimeMessages,
  STREAMING_TAIL_ID,
  streamingTailMessage,
  toThreadMessageLike,
} from '../assistantUiMessages';

function msg(over: Partial<ThreadMessage> = {}): ThreadMessage {
  return {
    id: 'm1',
    content: 'hello',
    type: 'text',
    extraMetadata: {},
    sender: 'user',
    createdAt: '2026-01-01T00:00:00.000Z',
    ...over,
  };
}

function tool(over: Partial<ToolTimelineEntry> = {}): ToolTimelineEntry {
  return { id: 'call-1', name: 'web_search', round: 1, seq: 0, status: 'running', ...over };
}

describe('toThreadMessageLike', () => {
  it('maps sender to role', () => {
    expect(toThreadMessageLike(msg({ id: 'u' })).role).toBe('user');
    expect(toThreadMessageLike(msg({ id: 'a', sender: 'agent' })).role).toBe('assistant');
  });

  it('unwraps a tool-call envelope so raw JSON never reaches the runtime', () => {
    const envelope = JSON.stringify({
      content: 'Pulling that up now.',
      tool_calls: [{ id: 'c1', name: 'memory_search', arguments: '{}' }],
    });
    const converted = toThreadMessageLike(msg({ id: 'e', sender: 'agent', content: envelope }));
    expect(converted.content).toEqual([{ type: 'text', text: 'Pulling that up now.' }]);
  });

  it('leaves ordinary prose untouched', () => {
    const m = msg({ id: 'p', sender: 'agent', content: 'just prose { not json' });
    expect(toThreadMessageLike(m).content).toEqual([
      { type: 'text', text: 'just prose { not json' },
    ]);
  });

  it('yields an empty content array for an empty message', () => {
    expect(toThreadMessageLike(msg({ id: 'blank', content: '' })).content).toEqual([]);
  });

  it('carries extraMetadata through as custom metadata', () => {
    const m = msg({ id: 'meta', extraMetadata: { requestId: 'r1' } });
    expect(toThreadMessageLike(m).metadata?.custom).toMatchObject({
      extraMetadata: { requestId: 'r1' },
    });
  });

  it('returns the identical object for the same source message', () => {
    const m = msg({ id: 'cached' });
    expect(toThreadMessageLike(m)).toBe(toThreadMessageLike(m));
  });
});

describe('streamingTailMessage', () => {
  it('is null with no stream and with an empty stream', () => {
    expect(streamingTailMessage(null)).toBeNull();
    expect(streamingTailMessage({ requestId: 'r', content: '', thinking: '' })).toBeNull();
  });

  it('is a running assistant message when tokens have landed', () => {
    const tail = streamingTailMessage({ requestId: 'r', content: 'partial', thinking: '' });
    expect(tail).toMatchObject({
      id: STREAMING_TAIL_ID,
      role: 'assistant',
      status: { type: 'running' },
      content: [{ type: 'text', text: 'partial' }],
    });
  });

  it('projects streamed thinking as a reasoning part before visible text', () => {
    const tail = streamingTailMessage({ requestId: 'r', content: 'answer', thinking: 'reasoning' });
    expect(tail?.content).toEqual([
      { type: 'reasoning', text: 'reasoning' },
      { type: 'text', text: 'answer' },
    ]);
  });

  it('keeps a running delegation on args and adds result only when complete', () => {
    const subagent = {
      taskId: 'sub-1',
      agentId: 'researcher',
      toolCalls: [],
      transcript: [{ kind: 'thinking' as const, text: 'checking sources' }],
    };
    const running = streamingTailMessage(null, [
      tool({ id: 'sub-1', name: 'subagent:researcher', subagent }),
    ]);
    const runningPart = running?.content[0];
    expect(runningPart).toMatchObject({
      type: 'tool-call',
      toolName: 'task',
      args: { progress: subagent },
    });
    expect(runningPart).not.toHaveProperty('result');

    const complete = streamingTailMessage(null, [
      tool({ id: 'sub-1', name: 'subagent:researcher', status: 'success', subagent }),
    ]);
    expect(complete?.content[0]).toMatchObject({
      type: 'tool-call',
      toolName: 'task',
      result: subagent,
    });
  });
});

describe('buildRuntimeMessages', () => {
  it('omits hidden messages', () => {
    const visible = msg({ id: 'v' });
    const hidden = msg({ id: 'h', extraMetadata: { hidden: true } });
    expect(buildRuntimeMessages([visible, hidden], null).map(m => m.id)).toEqual(['v']);
  });

  it('appends the live tail after the settled transcript', () => {
    const ids = buildRuntimeMessages([msg({ id: 'a' })], {
      requestId: 'r',
      content: 'tok',
      thinking: '',
    }).map(m => m.id);
    expect(ids).toEqual(['a', STREAMING_TAIL_ID]);
  });

  it('replays a settled turn reasoning and tool calls from its request id', () => {
    const answer = msg({
      id: 'answer',
      sender: 'agent',
      content: 'finished',
      extraMetadata: { requestId: 'req-1' },
    });
    const timeline = [tool({ id: 'call-1', status: 'success', result: 'found it' })];
    const transcript = [
      { kind: 'thinking' as const, round: 1, seq: 0, text: 'need to search' },
      { kind: 'toolCall' as const, round: 1, seq: 1, callId: 'call-1' },
    ];

    expect(
      buildRuntimeMessages([answer], null, {
        turnTimelines: { 'req-1': timeline },
        turnTranscripts: { 'req-1': transcript },
      })[0]?.content
    ).toEqual([
      { type: 'reasoning', text: 'need to search' },
      expect.objectContaining({
        type: 'tool-call',
        toolCallId: 'call-1',
        toolName: 'web_search',
        result: 'found it',
      }),
      { type: 'text', text: 'finished' },
    ]);
  });

  /**
   * The crash this guards: assistant-ui keys tool parts as `toolCallId-${id}`
   * and throws "Duplicate key … in useResources" on a repeat, taking the whole
   * thread render down on load. A provider that emits tool calls without ids
   * writes `''` for every one, so a settled turn can hold two transcript
   * pointers naming the same row.
   */
  it('emits one tool part per row when a turn has two pointers to the same call id', () => {
    const answer = msg({
      id: 'answer',
      sender: 'agent',
      content: 'done',
      extraMetadata: { requestId: 'req-1' },
    });
    const timeline = [tool({ id: '', status: 'success', result: 'once' })];
    const transcript = [
      { kind: 'toolCall' as const, round: 1, seq: 0, callId: '' },
      { kind: 'toolCall' as const, round: 1, seq: 1, callId: '' },
    ];

    const content = buildRuntimeMessages([answer], null, {
      turnTimelines: { 'req-1': timeline },
      turnTranscripts: { 'req-1': transcript },
    })[0]?.content as unknown as { type: string; toolCallId?: string }[];

    const toolIds = content.filter(part => part.type === 'tool-call').map(part => part.toolCallId);
    expect(toolIds).toHaveLength(1);
  });

  it('never repeats a toolCallId across the transcript and timeline passes', () => {
    const answer = msg({
      id: 'answer',
      sender: 'agent',
      content: 'done',
      extraMetadata: { requestId: 'req-1' },
    });
    const timeline = [
      tool({ id: 'c1', status: 'success', result: 'a' }),
      tool({ id: 'c2', status: 'success', result: 'b' }),
    ];
    const transcript = [{ kind: 'toolCall' as const, round: 1, seq: 0, callId: 'c1' }];

    const content = buildRuntimeMessages([answer], null, {
      turnTimelines: { 'req-1': timeline },
      turnTranscripts: { 'req-1': transcript },
    })[0]?.content as unknown as { type: string; toolCallId?: string }[];

    const toolIds = content.filter(part => part.type === 'tool-call').map(part => part.toolCallId);
    expect(toolIds).toEqual(['c1', 'c2']);
    expect(new Set(toolIds).size).toBe(toolIds.length);
  });

  it('re-converts only the tail as tokens land, never the settled transcript', () => {
    // The projection-level statement of the property `ChatThreadView.renderPerf`
    // pins for the render tree: streaming must not sweep the transcript.
    const settled = Array.from({ length: 40 }, (_, i) =>
      msg({ id: `m-${i}`, sender: i % 2 ? 'agent' : 'user', content: `prose ${i}` })
    );
    const parse = vi.spyOn(JSON, 'parse');

    buildRuntimeMessages(settled, null); // warm the identity cache
    parse.mockClear();

    let text = '';
    for (let i = 0; i < 5; i += 1) {
      text += ` tok${i}`;
      buildRuntimeMessages(settled, { requestId: 'r', content: text, thinking: '' });
    }

    // Zero: settled messages are cached by identity and the tail is plain text.
    expect(parse).not.toHaveBeenCalled();
    parse.mockRestore();
  });
});
