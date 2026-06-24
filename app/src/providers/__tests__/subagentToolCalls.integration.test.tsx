/**
 * Save + render integration for sub-agent tool calls.
 *
 * Drives the real socket-event listeners (`ChatRuntimeProvider`) against the
 * real Redux store, then renders the real `SubagentDrawer` from the resulting
 * state — so a regression in *either* the save path (events → Redux transcript)
 * or the render path (transcript → drawer rows) fails here. Covers the two
 * shapes a user sees: one sub-agent with several tool calls, and several
 * sub-agents each with their own calls.
 */
import { render, screen, within } from '@testing-library/react';
import { act } from 'react';
import { Provider } from 'react-redux';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import * as chatService from '../../services/chatService';
import { SubagentDrawer } from '../../pages/conversations/components/SubagentDrawer';
import { store } from '../../store';
import {
  clearAllChatRuntime,
  resetSessionTokenUsage,
  type SubagentActivity,
  type ToolTimelineEntryStatus,
} from '../../store/chatRuntimeSlice';
import { setStatusForUser } from '../../store/socketSlice';
import { clearAllThreads } from '../../store/threadSlice';
import ChatRuntimeProvider from '../ChatRuntimeProvider';

vi.mock('../../services/chatService', async () => {
  const actual = await vi.importActual<typeof chatService>('../../services/chatService');
  return { ...actual, subscribeChatEvents: vi.fn() };
});

vi.mock('../../services/api/threadApi', () => ({ threadApi: { getThreadMessages: vi.fn() } }));

vi.mock('../../hooks/usageRefresh', () => ({ requestUsageRefresh: vi.fn() }));
vi.mock('../../hooks/useRefetchSnapshotOnTurnEnd', () => ({
  useRefetchSnapshotOnTurnEnd: () => ({ refetch: vi.fn() }),
}));

function renderProvider(): chatService.ChatEventListeners {
  let captured: chatService.ChatEventListeners = {};
  vi.mocked(chatService.subscribeChatEvents).mockImplementation(listeners => {
    captured = listeners;
    return () => {};
  });
  store.dispatch(setStatusForUser({ userId: '__pending__', status: 'connected' }));
  render(
    <Provider store={store}>
      <ChatRuntimeProvider>
        <div />
      </ChatRuntimeProvider>
    </Provider>
  );
  return captured;
}

/** Read a sub-agent's accumulated activity straight from the live store. */
function subagentFromStore(threadId: string, taskId: string): SubagentActivity | undefined {
  return store
    .getState()
    .chatRuntime.toolTimelineByThread[threadId]?.find(e => e.subagent?.taskId === taskId)?.subagent;
}

function renderDrawer(subagent: SubagentActivity, status: ToolTimelineEntryStatus) {
  return render(
    <Provider store={store}>
      <SubagentDrawer subagent={subagent} status={status} onClose={() => {}} />
    </Provider>
  );
}

describe('sub-agent tool calls — save + render', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    store.dispatch(clearAllThreads());
    store.dispatch(clearAllChatRuntime());
    store.dispatch(resetSessionTokenUsage());
    store.dispatch(setStatusForUser({ userId: '__pending__', status: 'disconnected' }));
  });

  it('saves and renders multiple tool calls for a single sub-agent', () => {
    const listeners = renderProvider();
    const threadId = 't-multi';

    act(() => {
      listeners.onSubagentSpawned?.({
        thread_id: threadId,
        request_id: 'r1',
        tool_name: 'researcher',
        skill_id: 'sub-1',
        message: 'spawned',
        round: 1,
        subagent: { mode: 'typed', dedicated_thread: false, prompt_chars: 12 },
      });
      // Two distinct child tool calls.
      listeners.onSubagentToolCall?.({
        thread_id: threadId,
        request_id: 'r1',
        round: 1,
        tool_name: 'web_search',
        skill_id: 'sub-1',
        tool_call_id: 'c1',
        args: { query: 'rust async traits' },
        subagent: { agent_id: 'researcher', task_id: 'sub-1', child_iteration: 1 },
      });
      listeners.onSubagentToolCall?.({
        thread_id: threadId,
        request_id: 'r1',
        round: 1,
        tool_name: 'web_fetch',
        skill_id: 'sub-1',
        tool_call_id: 'c2',
        args: { url: 'https://github.com/foo/bar' },
        subagent: { agent_id: 'researcher', task_id: 'sub-1', child_iteration: 1 },
      });
      // Resolve both.
      listeners.onSubagentToolResult?.({
        thread_id: threadId,
        request_id: 'r1',
        round: 1,
        tool_name: 'web_search',
        skill_id: 'sub-1',
        tool_call_id: 'c1',
        success: true,
        output: 'search results',
        subagent: { agent_id: 'researcher', task_id: 'sub-1', elapsed_ms: 1200, output_chars: 14 },
      });
      listeners.onSubagentToolResult?.({
        thread_id: threadId,
        request_id: 'r1',
        round: 1,
        tool_name: 'web_fetch',
        skill_id: 'sub-1',
        tool_call_id: 'c2',
        success: true,
        output: 'page body',
        subagent: { agent_id: 'researcher', task_id: 'sub-1', elapsed_ms: 800, output_chars: 9 },
      });
    });

    // SAVE: both calls are in the transcript the drawer renders, resolved.
    const sub = subagentFromStore(threadId, 'sub-1');
    expect(sub).toBeDefined();
    const toolItems = (sub?.transcript ?? []).filter(i => i.kind === 'tool');
    expect(toolItems).toHaveLength(2);
    expect(toolItems.map(i => (i.kind === 'tool' ? i.toolName : ''))).toEqual([
      'web_search',
      'web_fetch',
    ]);
    expect(toolItems.every(i => i.kind === 'tool' && i.status === 'success')).toBe(true);

    // RENDER: the drawer shows both, with args-enriched labels.
    renderDrawer(sub!, 'success');
    const rows = screen.getAllByTestId('subagent-drawer-tool-call');
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('Searching: rust async traits');
    expect(rows[1].textContent).toContain('Fetching github.com');
  });

  it('saves and renders calls for two separate sub-agents independently', () => {
    const listeners = renderProvider();
    const threadId = 't-two-agents';

    const spawn = (taskId: string, agentId: string) =>
      listeners.onSubagentSpawned?.({
        thread_id: threadId,
        request_id: 'r1',
        tool_name: agentId,
        skill_id: taskId,
        message: 'spawned',
        round: 1,
        subagent: { mode: 'typed', dedicated_thread: false, prompt_chars: 5 },
      });
    const call = (taskId: string, agentId: string, tool: string, id: string, args: unknown) =>
      listeners.onSubagentToolCall?.({
        thread_id: threadId,
        request_id: 'r1',
        round: 1,
        tool_name: tool,
        skill_id: taskId,
        tool_call_id: id,
        args,
        subagent: { agent_id: agentId, task_id: taskId, child_iteration: 1 },
      });

    act(() => {
      spawn('sub-a', 'researcher');
      spawn('sub-b', 'coder');
      call('sub-a', 'researcher', 'web_search', 'a1', { query: 'topic A' });
      call('sub-b', 'coder', 'file_write', 'b1', { path: '/repo/src/x.ts' });
    });

    // Each sub-agent keeps only its own call.
    const a = subagentFromStore(threadId, 'sub-a');
    const b = subagentFromStore(threadId, 'sub-b');
    expect((a?.transcript ?? []).filter(i => i.kind === 'tool')).toHaveLength(1);
    expect((b?.transcript ?? []).filter(i => i.kind === 'tool')).toHaveLength(1);

    // Render each drawer; each shows exactly its own call.
    const { unmount } = renderDrawer(a!, 'running');
    let drawer = screen.getByTestId('subagent-drawer');
    expect(within(drawer).getAllByTestId('subagent-drawer-tool-call')).toHaveLength(1);
    expect(drawer.textContent).toContain('Searching: topic A');
    unmount();

    renderDrawer(b!, 'running');
    drawer = screen.getByTestId('subagent-drawer');
    expect(within(drawer).getAllByTestId('subagent-drawer-tool-call')).toHaveLength(1);
    expect(drawer.textContent).toContain('Writing file');
  });

  it('keeps a tool call that arrives before its spawn event (no drop)', () => {
    const listeners = renderProvider();
    const threadId = 't-race';

    act(() => {
      // Tool call first (race), spawn second.
      listeners.onSubagentToolCall?.({
        thread_id: threadId,
        request_id: 'r1',
        round: 1,
        tool_name: 'web_search',
        skill_id: 'sub-x',
        tool_call_id: 'x1',
        args: { query: 'early call' },
        subagent: { agent_id: 'researcher', task_id: 'sub-x', child_iteration: 1 },
      });
      listeners.onSubagentSpawned?.({
        thread_id: threadId,
        request_id: 'r1',
        tool_name: 'researcher',
        skill_id: 'sub-x',
        message: 'spawned',
        round: 1,
        subagent: { mode: 'typed', dedicated_thread: false, display_name: 'Researcher' },
      });
    });

    const sub = subagentFromStore(threadId, 'sub-x');
    expect((sub?.transcript ?? []).filter(i => i.kind === 'tool')).toHaveLength(1);
    renderDrawer(sub!, 'running');
    expect(screen.getByTestId('subagent-drawer').textContent).toContain('Searching: early call');
  });
});
