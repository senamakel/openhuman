import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ChatWindow } from '../../lib/orchestration/useOrchestrationChats';
import OrchestrationFocusPane, { type OrchestrationFocusPaneProps } from './OrchestrationFocusPane';

vi.mock('../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (k: string) => k }) }));

const chat = (over: Partial<ChatWindow>): ChatWindow =>
  ({
    id: 'sess-1',
    kind: 'session',
    title: 'Worker Alpha',
    subtitle: 'sub',
    pinned: false,
    active: true,
    unread: 0,
    messages: [],
    lastTimestamp: '2026-07-01T12:00:00.000Z',
    ...over,
  }) as ChatWindow;

let refresh: ReturnType<typeof vi.fn>;
const props = (over: Partial<OrchestrationFocusPaneProps>): OrchestrationFocusPaneProps =>
  ({
    selected: chat({}),
    sessionsState: { status: 'ok' },
    messagesState: { status: 'ok' },
    masterError: null,
    refresh,
    canCompose: false,
    composerBody: '',
    onComposerChange: vi.fn(),
    sending: false,
    onSubmitComposer: vi.fn(),
    ...over,
  }) as OrchestrationFocusPaneProps;

describe('OrchestrationFocusPane', () => {
  beforeEach(() => {
    refresh = vi.fn();
  });

  it('renders the payment-required state', () => {
    render(
      <OrchestrationFocusPane {...props({ sessionsState: { status: 'payment_required' } })} />
    );
    expect(screen.getByText('tinyplaceOrchestration.paymentRequired')).toBeInTheDocument();
  });

  it('renders a sessions load error and retries', () => {
    render(
      <OrchestrationFocusPane
        {...props({ sessionsState: { status: 'error', message: 'rpc down' } })}
      />
    );
    expect(screen.getByText(/tinyplaceOrchestration.failedToLoad/)).toBeInTheDocument();
    expect(screen.getByText(/rpc down/)).toBeInTheDocument();
    fireEvent.click(screen.getByText('common.retry'));
    expect(refresh).toHaveBeenCalled();
  });

  it('renders a messages load error', () => {
    render(
      <OrchestrationFocusPane
        {...props({ messagesState: { status: 'error', message: 'msg boom' } })}
      />
    );
    expect(screen.getByText(/msg boom/)).toBeInTheDocument();
  });

  it('does not render the retired steering directive controls', () => {
    render(
      <OrchestrationFocusPane
        {...props({ selected: chat({ kind: 'subconscious', pinned: true }) })}
      />
    );
    expect(screen.queryByTestId('tinyplace-steering-header')).not.toBeInTheDocument();
  });

  it('surfaces a composer send error when composing', () => {
    render(<OrchestrationFocusPane {...props({ canCompose: true, masterError: 'send failed' })} />);
    expect(screen.getByTestId('tinyplace-master-composer-input')).toBeInTheDocument();
    expect(screen.getByText(/send failed/)).toBeInTheDocument();
  });

  it('renders message bubbles for the selected chat', () => {
    render(
      <OrchestrationFocusPane
        {...props({
          selected: chat({
            messages: [
              {
                id: 'm1',
                from: '@peer',
                body: 'hi there',
                timestamp: '2026-07-01T12:00:00.000Z',
                encrypted: false,
              },
            ] as never,
          }),
        })}
      />
    );
    expect(
      within(screen.getByTestId('tinyplace-chat-messages')).getByText('hi there')
    ).toBeInTheDocument();
  });
});
