/**
 * End-to-end proof of the adapter seam: a component using assistant-ui's own
 * hooks, mounted under the runtime, must see exactly what Redux holds — and
 * must keep seeing it as Redux changes.
 *
 * This is the test that makes the adoption meaningful. The unit tests around
 * `assistantUiMessages` prove the projection; this proves the runtime consumes
 * it, so the runtime's view of the conversation and the store cannot drift.
 */
import { useAui, useAuiState } from '@assistant-ui/react';
import { combineReducers, configureStore } from '@reduxjs/toolkit';
import { act, render, screen } from '@testing-library/react';
import { Provider } from 'react-redux';
import { afterEach, describe, expect, it, vi } from 'vitest';

import chatRuntimeReducer, { streamDeltaReceived } from '../../store/chatRuntimeSlice';
import threadReducer from '../../store/threadSlice';
import type { ThreadMessage } from '../../types/thread';
import { AssistantUiRuntimeProvider } from '../AssistantUiRuntimeProvider';
import { __resetChatSurfaces, registerChatSurface } from '../chatSurfaceHandlers';

const THREAD_ID = 't-aui';

function msg(id: string, sender: 'user' | 'agent', content: string): ThreadMessage {
  return {
    id,
    sender,
    type: 'text',
    content,
    extraMetadata: {},
    createdAt: '2026-01-01T00:00:00.000Z',
  };
}

function buildStore(messages: ThreadMessage[]) {
  return configureStore({
    reducer: combineReducers({ thread: threadReducer, chatRuntime: chatRuntimeReducer }),
    preloadedState: {
      thread: {
        threads: [],
        selectedThreadId: THREAD_ID,
        activeThreadIds: {},
        welcomeThreadId: null,
        messagesByThreadId: { [THREAD_ID]: messages },
        messages: [],
        isLoadingThreads: false,
        isLoadingMessages: false,
        messagesError: null,
      },
    } as never,
  });
}

/** Renders what assistant-ui's runtime believes the thread contains. */
function RuntimeProbe() {
  const thread = useAuiState(({ thread: t }) => t);
  return (
    <div>
      <div data-testid="count">{thread.messages.length}</div>
      <div data-testid="running">{String(thread.isRunning)}</div>
      <div data-testid="text">
        {thread.messages
          .map(m => m.content.map(p => (p.type === 'text' ? p.text : '')).join(''))
          .join('|')}
      </div>
    </div>
  );
}

function renderWith(store: ReturnType<typeof buildStore>) {
  return render(
    <Provider store={store}>
      <AssistantUiRuntimeProvider>
        <RuntimeProbe />
      </AssistantUiRuntimeProvider>
    </Provider>
  );
}

afterEach(() => __resetChatSurfaces());

describe('AssistantUiRuntimeProvider', () => {
  it('exposes the Redux transcript through assistant-ui hooks', () => {
    renderWith(buildStore([msg('a', 'user', 'question'), msg('b', 'agent', 'answer')]));
    expect(screen.getByTestId('count')).toHaveTextContent('2');
    expect(screen.getByTestId('text')).toHaveTextContent('question|answer');
  });

  it('renders an empty thread without a live tail', () => {
    renderWith(buildStore([]));
    expect(screen.getByTestId('count')).toHaveTextContent('0');
  });

  it('surfaces the live stream as a running tail message', () => {
    const store = buildStore([msg('a', 'user', 'question')]);
    renderWith(store);
    expect(screen.getByTestId('count')).toHaveTextContent('1');

    act(() => {
      store.dispatch(
        streamDeltaReceived({
          threadId: THREAD_ID,
          requestId: 'req-1',
          round: 0,
          delta: 'partial answer',
          channel: 'content',
        })
      );
    });

    expect(screen.getByTestId('count')).toHaveTextContent('2');
    expect(screen.getByTestId('text')).toHaveTextContent('question|partial answer');
  });

  it('forwards onNew to the surface that owns the thread', async () => {
    const send = vi.fn(async () => {});
    registerChatSurface(THREAD_ID, { send });

    function Sender() {
      const aui = useAui();
      return (
        <button
          type="button"
          data-testid="send"
          onClick={() =>
            void aui.thread.append({ role: 'user', content: [{ type: 'text', text: 'hi' }] })
          }>
          send
        </button>
      );
    }

    render(
      <Provider store={buildStore([])}>
        <AssistantUiRuntimeProvider>
          <Sender />
        </AssistantUiRuntimeProvider>
      </Provider>
    );

    await act(async () => {
      screen.getByTestId('send').click();
    });

    expect(send).toHaveBeenCalledWith('hi');
  });
});
