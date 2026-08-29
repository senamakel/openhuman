import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { SessionSummary } from '../../lib/orchestration/orchestrationClient';
import ActiveSubagentsRail from './ActiveSubagentsRail';

// Pass-through translator so assertions can target the i18n keys directly.
vi.mock('../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (key: string) => key }) }));

function session(overrides: Partial<SessionSummary> = {}): SessionSummary {
  return {
    sessionId: 'sess-1',
    agentId: 'agent-1',
    label: 'Refactor the parser',
    status: 'running',
    active: true,
    messageCount: 3,
    unread: 0,
    ...overrides,
  } as SessionSummary;
}

const byContact = new Map<string, SessionSummary[]>([['0xabcdef0123456789', [session()]]]);

describe('ActiveSubagentsRail', () => {
  it('renders sub-agents under their instance and opens one on click', () => {
    const onOpenSession = vi.fn();
    render(
      <ActiveSubagentsRail
        byContact={byContact}
        openSessionId={null}
        isAgentTab={false}
        onOpenSession={onOpenSession}
      />
    );

    const row = screen.getByTestId('orch-session-rail-sess-1');
    expect(row).toHaveAttribute('data-slot', 'button');
    expect(row).toHaveAttribute('data-variant', 'tertiary');
    expect(screen.getByText('Refactor the parser')).toBeInTheDocument();

    fireEvent.click(row);
    expect(onOpenSession).toHaveBeenCalledWith('sess-1');
  });

  it('collapses the instance so its sub-agents disappear', () => {
    render(
      <ActiveSubagentsRail
        byContact={byContact}
        openSessionId={null}
        isAgentTab={false}
        onOpenSession={vi.fn()}
      />
    );

    const toggle = screen.getByTestId('orch-instance-toggle-0xabcdef0123456789');
    expect(toggle).toHaveAttribute('aria-expanded', 'true');

    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByTestId('orch-session-rail-sess-1')).not.toBeInTheDocument();
  });

  it('renders the empty copy when no instances exist', () => {
    render(
      <ActiveSubagentsRail
        byContact={new Map()}
        openSessionId={null}
        isAgentTab={false}
        onOpenSession={vi.fn()}
      />
    );
    expect(screen.getByText('orchPage.sessions.empty')).toBeInTheDocument();
  });
});
