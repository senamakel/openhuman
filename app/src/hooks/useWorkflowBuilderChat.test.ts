import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { BuilderTurnResult } from '../services/api/flowsApi';
import type { WorkflowProposal } from '../store/chatRuntimeSlice';
import { useWorkflowBuilderChat } from './useWorkflowBuilderChat';

// The hook now runs the builder server-side via `openhuman.flows_build`.
const buildWorkflow = vi.hoisted(() => vi.fn());
vi.mock('../services/api/flowsApi', () => ({ buildWorkflow }));

// Socket is always "connected" for these tests.
vi.mock('../store/socketSelectors', () => ({ selectSocketStatus: () => 'connected' }));

const dispatch = vi.hoisted(() => vi.fn());
const selectorState = vi.hoisted(() => ({
  proposals: {} as Record<string, WorkflowProposal>,
  messagesByThreadId: {} as Record<string, unknown[]>,
  toolTimelineByThread: {} as Record<string, unknown[]>,
  streamingAssistantByThread: {} as Record<string, { content: string }>,
}));
vi.mock('../store/hooks', () => ({
  useAppDispatch: () => dispatch,
  useAppSelector: (sel: (s: unknown) => unknown) =>
    sel({
      thread: { messagesByThreadId: selectorState.messagesByThreadId },
      chatRuntime: {
        pendingWorkflowProposalsByThread: selectorState.proposals,
        toolTimelineByThread: selectorState.toolTimelineByThread,
        streamingAssistantByThread: selectorState.streamingAssistantByThread,
      },
    }),
}));

vi.mock('../store/threadSlice', () => ({
  createNewThread: (labels: string[]) => ({ type: 'createNewThread', labels }),
  addMessageLocal: (p: unknown) => ({ type: 'addMessageLocal', p }),
}));
vi.mock('../store/chatRuntimeSlice', () => ({
  clearWorkflowProposalForThread: (p: unknown) => ({ type: 'clearProposal', p }),
  setWorkflowProposalForThread: (p: unknown) => ({ type: 'setProposal', p }),
}));

const okResult = (over: Partial<BuilderTurnResult> = {}): BuilderTurnResult => ({
  proposal: null,
  assistantText: 'done',
  error: null,
  ...over,
});

describe('useWorkflowBuilderChat', () => {
  beforeEach(() => {
    buildWorkflow.mockReset().mockResolvedValue(okResult());
    selectorState.proposals = {};
    selectorState.messagesByThreadId = {};
    selectorState.toolTimelineByThread = {};
    selectorState.streamingAssistantByThread = {};
    dispatch.mockReset().mockImplementation((action: { type: string }) => {
      if (action.type === 'createNewThread') {
        return { unwrap: () => Promise.resolve({ id: 'builder-1' }) };
      }
      if (action.type === 'addMessageLocal') {
        return { unwrap: () => Promise.resolve(undefined) };
      }
      return undefined;
    });
  });

  it('creates a dedicated thread on first send and runs the builder there', async () => {
    const { result } = renderHook(() => useWorkflowBuilderChat());
    expect(result.current.threadId).toBeNull();

    await act(async () => {
      await result.current.send({
        displayText: 'hi',
        request: { mode: 'create', instruction: 'email me a digest' },
      });
    });

    // A dedicated "workflow-builder" thread was created and the agent run there.
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'createNewThread', labels: ['workflow-builder'] })
    );
    // The builder turn streams onto the dedicated thread — its id is threaded
    // into `flows_build` as the second arg.
    expect(buildWorkflow).toHaveBeenCalledWith(
      { mode: 'create', instruction: 'email me a digest' },
      'builder-1'
    );
    await waitFor(() => expect(result.current.threadId).toBe('builder-1'));
  });

  it('surfaces the proposal the builder returned by dispatching it into the store', async () => {
    const proposal: WorkflowProposal = {
      name: 'Digest',
      graph: { nodes: [], edges: [] },
      requireApproval: true,
      summary: { trigger: 'schedule', steps: [] },
    };
    buildWorkflow.mockResolvedValue(okResult({ proposal }));

    const { result } = renderHook(() => useWorkflowBuilderChat());
    await act(async () => {
      await result.current.send({
        displayText: 'hi',
        request: { mode: 'create', instruction: 'x' },
      });
    });

    // The proposal is written into the shared store slice via setProposal.
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'setProposal', p: { threadId: 'builder-1', proposal } })
    );
  });

  it('appends only the user turn locally — the runtime owns the agent reply', async () => {
    buildWorkflow.mockResolvedValue(okResult({ assistantText: 'Here is your workflow.' }));
    const { result } = renderHook(() => useWorkflowBuilderChat());
    await act(async () => {
      await result.current.send({
        displayText: 'hi',
        request: { mode: 'create', instruction: 'x' },
      });
    });
    const appended = dispatch.mock.calls
      .map(([a]) => a as { type: string; p?: { message?: { sender?: string } } })
      .filter(a => a.type === 'addMessageLocal');
    // The web channel never persists user messages, so the hook appends the
    // user turn itself...
    expect(appended.some(a => a.p?.message?.sender === 'user')).toBe(true);
    // ...but NOT the agent reply — `ChatRuntimeProvider` appends that on the
    // streamed `chat_done`, so appending here too would double it.
    expect(appended.some(a => a.p?.message?.sender === 'agent')).toBe(false);
  });

  it('reuses the same dedicated thread across sends (creates it once)', async () => {
    const { result } = renderHook(() => useWorkflowBuilderChat());
    await act(async () => {
      await result.current.send({
        displayText: 'one',
        request: { mode: 'create', instruction: 'a' },
      });
    });
    await act(async () => {
      await result.current.send({
        displayText: 'two',
        request: { mode: 'revise', instruction: 'b' },
      });
    });
    const createCalls = dispatch.mock.calls.filter(
      ([a]) => (a as { type: string }).type === 'createNewThread'
    );
    expect(createCalls).toHaveLength(1);
    expect(buildWorkflow).toHaveBeenLastCalledWith(
      { mode: 'revise', instruction: 'b' },
      'builder-1'
    );
  });

  it('surfaces the streamed tool timeline + live response for the dedicated thread', async () => {
    const { result } = renderHook(() => useWorkflowBuilderChat());
    await act(async () => {
      await result.current.send({
        displayText: 'hi',
        request: { mode: 'create', instruction: 'x' },
      });
    });
    // Simulate the runtime streaming onto this thread, then re-render.
    selectorState.toolTimelineByThread = {
      'builder-1': [{ id: 't1', name: 'propose_workflow', round: 0, status: 'running' }],
    };
    selectorState.streamingAssistantByThread = { 'builder-1': { content: 'drafting…' } };
    const { result: result2 } = renderHook(() => useWorkflowBuilderChat('builder-1'));
    expect(result2.current.toolTimeline).toHaveLength(1);
    expect(result2.current.liveResponse).toBe('drafting…');
  });

  it('sets an error when the builder run fails without a proposal', async () => {
    buildWorkflow.mockResolvedValue(okResult({ error: 'run failed', assistantText: '' }));
    const { result } = renderHook(() => useWorkflowBuilderChat());
    await act(async () => {
      await result.current.send({
        displayText: 'hi',
        request: { mode: 'create', instruction: 'x' },
      });
    });
    await waitFor(() => expect(result.current.error).toBe('run failed'));
  });
});
