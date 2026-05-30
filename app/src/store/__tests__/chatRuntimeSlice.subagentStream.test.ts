import { describe, expect, it } from 'vitest';

import reducer, {
  appendSubagentStreamDelta,
  setToolTimelineForThread,
  type ToolTimelineEntry,
} from '../chatRuntimeSlice';

const THREAD = 'thread-1';
const ROW_ID = `${THREAD}:subagent:sub-1:researcher`;

function withSubagentRow(): ReturnType<typeof reducer> {
  const entry: ToolTimelineEntry = {
    id: ROW_ID,
    name: 'subagent:researcher',
    round: 1,
    status: 'running',
    subagent: {
      taskId: 'sub-1',
      agentId: 'researcher',
      toolCalls: [],
      streamingText: '',
      streamingThinking: '',
    },
  };
  return reducer(undefined, setToolTimelineForThread({ threadId: THREAD, entries: [entry] }));
}

describe('appendSubagentStreamDelta', () => {
  it('accumulates text deltas in order on the matching row', () => {
    let state = withSubagentRow();
    state = reducer(
      state,
      appendSubagentStreamDelta({ threadId: THREAD, rowId: ROW_ID, kind: 'text', delta: 'Hello ' })
    );
    state = reducer(
      state,
      appendSubagentStreamDelta({ threadId: THREAD, rowId: ROW_ID, kind: 'text', delta: 'world' })
    );
    const row = state.toolTimelineByThread[THREAD][0];
    expect(row.subagent?.streamingText).toBe('Hello world');
    expect(row.subagent?.streamingThinking).toBe('');
  });

  it('accumulates thinking deltas independently of text', () => {
    let state = withSubagentRow();
    state = reducer(
      state,
      appendSubagentStreamDelta({
        threadId: THREAD,
        rowId: ROW_ID,
        kind: 'thinking',
        delta: 'reasoning…',
      })
    );
    state = reducer(
      state,
      appendSubagentStreamDelta({ threadId: THREAD, rowId: ROW_ID, kind: 'text', delta: 'answer' })
    );
    const sub = state.toolTimelineByThread[THREAD][0].subagent;
    expect(sub?.streamingThinking).toBe('reasoning…');
    expect(sub?.streamingText).toBe('answer');
  });

  it('is a no-op when the thread or row is unknown', () => {
    const state = withSubagentRow();
    const unknownThread = reducer(
      state,
      appendSubagentStreamDelta({ threadId: 'nope', rowId: ROW_ID, kind: 'text', delta: 'x' })
    );
    expect(unknownThread).toEqual(state);

    const unknownRow = reducer(
      state,
      appendSubagentStreamDelta({ threadId: THREAD, rowId: 'missing', kind: 'text', delta: 'x' })
    );
    expect(unknownRow.toolTimelineByThread[THREAD][0].subagent?.streamingText).toBe('');
  });

  it('tolerates an undefined starting buffer (legacy row)', () => {
    const entry: ToolTimelineEntry = {
      id: ROW_ID,
      name: 'subagent:researcher',
      round: 1,
      status: 'running',
      subagent: { taskId: 'sub-1', agentId: 'researcher', toolCalls: [] },
    };
    let state = reducer(
      undefined,
      setToolTimelineForThread({ threadId: THREAD, entries: [entry] })
    );
    state = reducer(
      state,
      appendSubagentStreamDelta({ threadId: THREAD, rowId: ROW_ID, kind: 'text', delta: 'hi' })
    );
    expect(state.toolTimelineByThread[THREAD][0].subagent?.streamingText).toBe('hi');
  });
});
