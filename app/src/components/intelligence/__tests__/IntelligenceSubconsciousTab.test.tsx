/**
 * Vitest for the Intelligence Subconscious tab.
 *
 * Covers navigation from reflection cards and provider unavailable state.
 */
import { fireEvent, render, screen } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { setSelectedThread } from '../../../store/threadSlice';
import IntelligenceSubconsciousTab from '../IntelligenceSubconsciousTab';

const mockDispatch = vi.fn();
const mockNavigate = vi.fn();

vi.mock('react-redux', () => ({ useDispatch: () => mockDispatch, useSelector: () => 'en' }));

vi.mock('react-router-dom', () => ({ useNavigate: () => mockNavigate }));

vi.mock('../SubconsciousReflectionCards', () => ({
  default: ({ onNavigateToThread }: { onNavigateToThread?: (id: string) => void }) => (
    <button
      type="button"
      data-testid="cards-stub-trigger"
      onClick={() => onNavigateToThread?.('spawned-thread-42')}>
      trigger
    </button>
  ),
}));

function baseProps(): ComponentProps<typeof IntelligenceSubconsciousTab> {
  return {
    status: null,
    triggerTick: vi.fn(),
    triggering: false,
  };
}

describe('IntelligenceSubconsciousTab', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('on Act → dispatches setSelectedThread + navigates to /chat', () => {
    render(<IntelligenceSubconsciousTab {...baseProps()} />);
    fireEvent.click(screen.getByTestId('cards-stub-trigger'));
    expect(mockDispatch).toHaveBeenCalledWith(setSelectedThread('spawned-thread-42'));
    expect(mockNavigate).toHaveBeenCalledWith('/chat');
  });

  it('shows provider unavailable state and blocks manual ticks', () => {
    const triggerTick = vi.fn();
    render(
      <IntelligenceSubconsciousTab
        {...baseProps()}
        triggerTick={triggerTick}
        status={{
          enabled: true,
          provider_available: false,
          provider_unavailable_reason: 'Sign in or configure a local Subconscious provider.',
          interval_minutes: 5,
          last_tick_at: null,
          total_ticks: 0,
          consecutive_failures: 1,
        }}
      />
    );

    expect(screen.getByText('Subconscious is paused')).toBeInTheDocument();
    expect(screen.getByText(/configure a local Subconscious provider/i)).toBeInTheDocument();

    const runNow = screen.getByRole('button', { name: /Run Now/i });
    expect(runNow).toBeDisabled();
    fireEvent.click(runNow);
    expect(triggerTick).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: /AI settings/i }));
    expect(mockNavigate).toHaveBeenCalledWith('/settings/llm');
  });
});
