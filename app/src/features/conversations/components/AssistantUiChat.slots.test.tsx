/**
 * Composer slot identity on the assistant-ui chat surface.
 *
 * `thread.tsx` renders each host slot **by type**, so a slot defined as a
 * callback with a changing dep list hands React a new element type and the
 * subtree unmounts and remounts on every host render (#6246). The slots now
 * hold a constant identity and read their props through a ref instead.
 *
 * That trade has a failure mode of its own: a frozen slot that never re-renders
 * would show stale props forever. It does not happen here because nothing in
 * `thread.tsx` is memoized — the whole subtree re-renders with its host — and
 * these tests pin that down, because it is the only thing keeping the stable
 * slots correct.
 */
import { combineReducers, configureStore } from '@reduxjs/toolkit';
import { fireEvent, render, screen } from '@testing-library/react';
import { Provider } from 'react-redux';
import { describe, expect, it, vi } from 'vitest';

import chatRuntimeReducer from '../../../store/chatRuntimeSlice';
import mascotReducer from '../../../store/mascotSlice';
import threadReducer from '../../../store/threadSlice';
import { AssistantUiChat } from './AssistantUiChat';
import type { ThreadGoalController } from './ThreadGoalChip';

const THREAD_ID = 't-slots';

function buildStore() {
  return configureStore({
    reducer: combineReducers({
      thread: threadReducer,
      chatRuntime: chatRuntimeReducer,
      mascot: mascotReducer,
    }),
    preloadedState: {
      thread: {
        threads: [
          {
            id: THREAD_ID,
            title: 'Slots thread',
            chatId: null,
            isActive: false,
            messageCount: 0,
            lastMessageAt: '2026-01-01T00:00:00.000Z',
            createdAt: '2026-01-01T00:00:00.000Z',
            labels: [],
          },
        ],
        selectedThreadId: THREAD_ID,
        activeThreadIds: {},
        welcomeThreadId: null,
        messagesByThreadId: { [THREAD_ID]: [] },
        messages: [],
        isLoadingThreads: false,
        isLoadingMessages: false,
        messagesError: null,
      },
    } as never,
  });
}

/**
 * A fresh controller object every time, the way `useThreadGoal` returns one:
 * it is a bare object literal, so its identity changes on every host render.
 */
function goalController(overrides: Partial<ThreadGoalController> = {}): ThreadGoalController {
  return {
    threadId: THREAD_ID,
    goal: null,
    expanded: false,
    draft: '',
    busy: false,
    setDraft: vi.fn(),
    open: vi.fn(),
    close: vi.fn(),
    toggle: vi.fn(),
    save: vi.fn(),
    complete: vi.fn(),
    pause: vi.fn(),
    resume: vi.fn(),
    clear: vi.fn(),
    ...overrides,
  };
}

function chat(
  threadGoal: ThreadGoalController,
  onOpenHumanMode?: () => void,
  overrides: { attachmentsEnabled?: boolean; attachmentInteractionBlocked?: boolean } = {}
) {
  return (
    <AssistantUiChat
      threadGoal={threadGoal}
      model={null}
      onModelChange={vi.fn()}
      inputValue=""
      onInputValueChange={vi.fn()}
      attachments={[]}
      onAttachFiles={vi.fn()}
      onRemoveAttachment={vi.fn()}
      maxAttachments={5}
      attachmentsEnabled={overrides.attachmentsEnabled ?? false}
      attachmentInteractionBlocked={overrides.attachmentInteractionBlocked ?? false}
      onAttachmentOnlySend={vi.fn()}
      onOpenHumanMode={onOpenHumanMode}
    />
  );
}

function composerShell(): HTMLElement {
  return document.querySelector('[data-slot="aui_composer-shell"]') as HTMLElement;
}

describe('assistant-ui composer slots', () => {
  it('shows the thread-goal editor once the controller expands', async () => {
    const store = buildStore();
    const { rerender } = render(<Provider store={store}>{chat(goalController())}</Provider>);

    expect(screen.queryByPlaceholderText('What should this thread accomplish?')).toBeNull();

    // The host re-renders with a new controller object carrying `expanded`.
    // `ComposerExtras` holds a constant identity and has no dependency on it,
    // so this only appears if the slot re-renders with its host.
    rerender(<Provider store={store}>{chat(goalController({ expanded: true }))}</Provider>);

    expect(
      await screen.findByPlaceholderText('What should this thread accomplish?')
    ).toBeInTheDocument();
  });

  it('keeps the idle mascot button mounted across a host re-render', () => {
    const store = buildStore();
    const navigate = vi.fn();
    // A fresh arrow per render, exactly as `Conversations` passes it
    // (`onOpenHumanMode={() => navigate('/human')}`). That changing identity is
    // what used to remount the slot.
    const { rerender } = render(
      <Provider store={store}>{chat(goalController(), () => navigate('/human'))}</Provider>
    );

    const button = screen.getByTestId('composer-human-mode');

    rerender(<Provider store={store}>{chat(goalController(), () => navigate('/human'))}</Provider>);

    expect(screen.getByTestId('composer-human-mode')).toBe(button);
  });

  it('refuses a file drag while the composer is locked', () => {
    const store = buildStore();
    render(
      <Provider store={store}>
        {chat(goalController(), undefined, {
          attachmentsEnabled: true,
          attachmentInteractionBlocked: true,
        })}
      </Provider>
    );

    const dataTransfer = { types: ['Files'], dropEffect: 'copy' };
    fireEvent.dragOver(composerShell(), { dataTransfer });

    // `preventDefault` still ran — otherwise the webview navigates away to the
    // dropped file — but the drop is refused and no affordance is shown.
    expect(dataTransfer.dropEffect).toBe('none');
    expect(composerShell().getAttribute('data-dragging')).toBeNull();
  });

  it('leaves the assistant-ui dropzone in charge when the host takes no files', () => {
    const store = buildStore();
    render(<Provider store={store}>{chat(goalController())}</Provider>);

    const dataTransfer = { types: ['Files'], dropEffect: 'copy' };
    fireEvent.dragOver(composerShell(), { dataTransfer });

    // `attachmentsEnabled` is false here, so no host file sink is published and
    // the primitive's own (capability-gated) handling is what remains.
    expect(composerShell().getAttribute('data-dragging')).toBeNull();
  });
});
