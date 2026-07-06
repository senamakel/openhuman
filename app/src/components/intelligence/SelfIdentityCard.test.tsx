import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { SelfIdentity } from '../../lib/orchestration/orchestrationClient';
import SelfIdentityCard from './SelfIdentityCard';

vi.mock('../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (k: string) => k }) }));

const discoverable: SelfIdentity = {
  agentId: '6wNaBJkatir4B86cw5ykHZWQ3xoNaKygX5vAU9MQbHSh',
  handles: [{ username: 'openhuman', primary: true }],
  primaryHandle: 'openhuman',
  cardPublished: true,
  keyPublished: true,
  discoverable: true,
};

describe('SelfIdentityCard', () => {
  beforeEach(() => {
    Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
  });

  it('shows a loading state before the identity resolves', () => {
    render(<SelfIdentityCard identity={null} loading />);
    expect(screen.getByTestId('tinyplace-self-identity')).toHaveTextContent(
      'tinyplaceOrchestration.identity.loading'
    );
  });

  it('renders nothing once loaded with no identity', () => {
    const { container } = render(<SelfIdentityCard identity={null} loading={false} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders the primary handle, shortened address and discoverable status', () => {
    render(<SelfIdentityCard identity={discoverable} loading={false} />);
    expect(screen.getByText('@openhuman')).toBeInTheDocument();
    // Address is shortened but the full value is preserved in the title.
    expect(screen.getByText('6wNaBJ…QbHSh')).toHaveAttribute('title', discoverable.agentId);
    const status = screen.getByTestId('tinyplace-self-identity-status');
    expect(status).toHaveAttribute('data-discoverable', 'true');
  });

  it('flags an un-messageable identity as undiscoverable without nagging to register', () => {
    const undiscoverable: SelfIdentity = {
      agentId: 'addrWithNoCardPublishedYet',
      handles: [],
      cardPublished: false,
      keyPublished: false,
      discoverable: false,
    };
    render(<SelfIdentityCard identity={undiscoverable} loading={false} />);
    expect(screen.getByText('tinyplaceOrchestration.identity.noHandle')).toBeInTheDocument();
    expect(screen.getByTestId('tinyplace-self-identity-status')).toHaveAttribute(
      'data-discoverable',
      'false'
    );
    // The prescriptive "register a handle" hint is intentionally gone — a handle
    // is optional, so the card reports status without nagging.
    expect(
      screen.queryByText('tinyplaceOrchestration.identity.undiscoverableHint')
    ).not.toBeInTheDocument();
  });

  it('copies the address to the clipboard on click', async () => {
    render(<SelfIdentityCard identity={discoverable} loading={false} />);
    fireEvent.click(screen.getByTestId('tinyplace-self-identity-copy'));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(discoverable.agentId);
    await waitFor(() =>
      expect(screen.getByTestId('tinyplace-self-identity-copy')).toHaveTextContent(
        'tinyplaceOrchestration.identity.copied'
      )
    );
  });
});
