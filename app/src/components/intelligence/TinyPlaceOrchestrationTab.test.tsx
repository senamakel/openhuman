import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { apiClient } from '../../agentworld/AgentWorldShell';
import TinyPlaceOrchestrationTab from './TinyPlaceOrchestrationTab';

vi.mock('../../agentworld/AgentWorldShell', () => ({
  apiClient: { messages: { list: vi.fn() }, inbox: { list: vi.fn() } },
}));

vi.mock('../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (k: string) => k }) }));

const messagesListMock = vi.mocked(apiClient.messages.list);
const inboxListMock = vi.mocked(apiClient.inbox.list);

describe('TinyPlaceOrchestrationTab', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    messagesListMock.mockResolvedValue({ messages: [] });
    inboxListMock.mockResolvedValue({ items: [], unreadCount: 0, totalCount: 0 });
  });

  it('renders pinned master and subconscious chats before session chats', async () => {
    messagesListMock.mockResolvedValue({
      messages: [
        {
          id: 'm-master',
          from: 'human',
          to: 'master-agent',
          timestamp: '2026-07-01T12:00:00.000Z',
          deviceId: 1,
          type: 'agent-human',
          body: 'Coordinate the next worker handoff',
        },
        {
          id: 'm-subconscious',
          from: 'subconscious-loop',
          to: 'tinyplace_agent',
          timestamp: '2026-07-01T12:01:00.000Z',
          deviceId: 1,
          type: 'internal',
          body: 'Memory synthesis finished',
        },
        {
          id: 'm-session',
          from: '@worker-alpha',
          to: '@openhuman',
          timestamp: '2026-07-01T12:02:00.000Z',
          deviceId: 1,
          type: 'session',
          body: 'I opened a worktree and started the review.',
          sessionId: 'app-session-1',
          sessionLabel: 'OpenHuman app session',
        },
      ],
    });

    render(<TinyPlaceOrchestrationTab />);

    expect(await screen.findAllByText('tinyplaceOrchestration.master.title')).toHaveLength(2);
    expect(screen.getByText('tinyplaceOrchestration.subconscious.title')).toBeInTheDocument();
    expect(screen.getByText('OpenHuman app session')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tinyplace-chat-session:app-session-1'));

    expect(
      within(await screen.findByTestId('tinyplace-chat-messages')).getByText(
        'I opened a worktree and started the review.'
      )
    ).toBeInTheDocument();
  });

  it('adds unread inbox sessions and marks them active', async () => {
    inboxListMock.mockResolvedValue({
      items: [
        {
          itemId: 'inbox-1',
          type: 'dm',
          status: 'unread',
          priority: 'normal',
          timestamp: '2026-07-01T12:03:00.000Z',
          subject: 'Worker update',
          summary: 'The subagent is waiting on a decision.',
          from: '@worker-beta',
        },
      ],
      unreadCount: 1,
      totalCount: 1,
    });

    render(<TinyPlaceOrchestrationTab />);

    expect(await screen.findByText('@worker-beta')).toBeInTheDocument();
    expect(screen.getByText('The subagent is waiting on a decision.')).toBeInTheDocument();
    expect(screen.getByText('1')).toBeInTheDocument();
    expect(screen.getByText('tinyplaceOrchestration.active')).toBeInTheDocument();
  });

  it('surfaces load errors and retries', async () => {
    messagesListMock.mockRejectedValueOnce(new Error('rpc failed'));

    render(<TinyPlaceOrchestrationTab />);

    expect(await screen.findByText(/tinyplaceOrchestration.failedToLoad/)).toBeInTheDocument();
    expect(screen.getByText(/rpc failed/)).toBeInTheDocument();

    fireEvent.click(screen.getByText('common.retry'));

    await waitFor(() => expect(messagesListMock).toHaveBeenCalledTimes(2));
    expect(await screen.findByText('tinyplaceOrchestration.noMessages')).toBeInTheDocument();
  });
});
