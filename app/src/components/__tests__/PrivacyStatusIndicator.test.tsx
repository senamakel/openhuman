import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { renderWithProviders } from '../../test/test-utils';
import PrivacyStatusIndicator from '../PrivacyStatusIndicator';

describe('PrivacyStatusIndicator', () => {
  it('renders nothing until the privacy mode is hydrated', () => {
    const { container } = renderWithProviders(<PrivacyStatusIndicator />, {
      preloadedState: { privacy: { privacyMode: null, activeExternalByThread: {} } },
    });
    expect(container.firstChild).toBeNull();
  });

  it('shows the mode + on-device state when no external transfer is active', () => {
    renderWithProviders(<PrivacyStatusIndicator />, {
      preloadedState: {
        privacy: { privacyMode: 'standard', activeExternalByThread: {} },
        thread: { selectedThreadId: 'thread-1' },
      },
    });
    const pill = screen.getByRole('status');
    expect(pill).toHaveTextContent('Standard');
    expect(pill).toHaveTextContent('On-device');
    expect(pill).toHaveAttribute('title', 'Standard · On-device');
  });

  it('shows the off-device state when the active thread has a live external transfer', () => {
    renderWithProviders(<PrivacyStatusIndicator />, {
      preloadedState: {
        privacy: { privacyMode: 'standard', activeExternalByThread: { 'thread-1': true } },
        thread: { selectedThreadId: 'thread-1' },
      },
    });
    const pill = screen.getByRole('status');
    expect(pill).toHaveTextContent('Off-device');
    expect(pill).toHaveAttribute('title', 'Standard · Off-device');
  });

  it('always reads on-device in local-only mode, even with a live external flag', () => {
    renderWithProviders(<PrivacyStatusIndicator />, {
      preloadedState: {
        privacy: { privacyMode: 'local_only', activeExternalByThread: { 'thread-1': true } },
        thread: { selectedThreadId: 'thread-1' },
      },
    });
    const pill = screen.getByRole('status');
    expect(pill).toHaveTextContent('Local-only');
    expect(pill).toHaveTextContent('On-device');
  });

  it('ignores a live external transfer that belongs to a different thread', () => {
    renderWithProviders(<PrivacyStatusIndicator />, {
      preloadedState: {
        privacy: { privacyMode: 'standard', activeExternalByThread: { 'other-thread': true } },
        thread: { selectedThreadId: 'thread-1' },
      },
    });
    expect(screen.getByRole('status')).toHaveTextContent('On-device');
  });
});
