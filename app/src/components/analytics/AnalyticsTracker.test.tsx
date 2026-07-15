import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, useNavigate } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import { AnalyticsPageTracker, trackAnalyticsEvent, TrackedInteraction } from './AnalyticsTracker';

const mocks = vi.hoisted(() => ({ trackEvent: vi.fn(), trackPageView: vi.fn() }));

vi.mock('../../services/analytics', () => ({
  trackEvent: mocks.trackEvent,
  trackPageView: mocks.trackPageView,
}));

describe('analytics tracking primitives', () => {
  it('tracks a page when its path changes', () => {
    function PageHarness() {
      const navigate = useNavigate();
      return (
        <>
          <AnalyticsPageTracker />
          <button type="button" onClick={() => navigate('/flows')}>
            Navigate
          </button>
        </>
      );
    }

    render(
      <MemoryRouter initialEntries={['/chat']}>
        <PageHarness />
      </MemoryRouter>
    );
    expect(mocks.trackPageView).toHaveBeenLastCalledWith('/chat');

    fireEvent.click(screen.getByRole('button', { name: 'Navigate' }));
    expect(mocks.trackPageView).toHaveBeenLastCalledWith('/flows');
  });

  it('adds a stable id and preserves the child click handler', () => {
    const onClick = vi.fn();
    render(
      <TrackedInteraction id="flows-run">
        <button type="button" onClick={onClick}>
          Run
        </button>
      </TrackedInteraction>
    );

    const button = screen.getByRole('button');
    fireEvent.click(button);

    expect(button).toHaveAttribute('data-analytics-id', 'flows-run');
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('can emit a typed semantic click event', () => {
    render(
      <TrackedInteraction
        id="connect-slack"
        event="account_connect_start"
        properties={{ provider: 'slack' }}>
        <button type="button">Connect</button>
      </TrackedInteraction>
    );

    fireEvent.click(screen.getByRole('button'));
    expect(mocks.trackEvent).toHaveBeenCalledWith('account_connect_start', { provider: 'slack' });
  });

  it('provides the same typed facade for successful non-click outcomes', () => {
    trackAnalyticsEvent('chat_message_sent', { send_mode: 'standard' });
    expect(mocks.trackEvent).toHaveBeenCalledWith('chat_message_sent', { send_mode: 'standard' });
  });
});
