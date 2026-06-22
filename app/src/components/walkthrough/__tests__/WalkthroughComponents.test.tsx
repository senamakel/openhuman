import { fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';

import type { WalkthroughState } from '../../../pages/onboarding/OnboardingContext';
import WalkthroughActionCard from '../WalkthroughActionCard';
import WalkthroughContainer from '../WalkthroughContainer';
import WalkthroughPhasePanel from '../WalkthroughPhasePanel';
import WalkthroughProgressBar from '../WalkthroughProgressBar';
import { WalkthroughProvider } from '../WalkthroughProvider';

vi.mock('../../../lib/i18n/I18nContext', () => ({
  useT: () => ({ t: (_key: string, fallback?: string) => fallback ?? _key }),
}));

const connectState: WalkthroughState = {
  phase: 'connect',
  steps: [
    { key: 'gmail', completed: false },
    { key: 'slack', completed: true },
  ],
  completed: false,
  skipped: false,
};

function renderWithProvider(
  state: WalkthroughState,
  ui: ReactNode,
  overrides: { onAdvance?: (stepKey?: string) => WalkthroughState; onSkip?: () => void } = {}
) {
  const onAdvance = overrides.onAdvance ?? vi.fn(() => state);
  const onSkip = overrides.onSkip ?? vi.fn();

  const result = render(
    <WalkthroughProvider state={state} onAdvance={onAdvance} onSkip={onSkip}>
      {ui}
    </WalkthroughProvider>
  );

  return { ...result, onAdvance, onSkip };
}

describe('walkthrough components', () => {
  it('renders action cards with labels, descriptions, and completion state', () => {
    const { onAdvance } = renderWithProvider(
      connectState,
      <WalkthroughActionCard step={connectState.steps[0]} />
    );

    const gmail = screen.getByRole('button', { name: 'Complete Gmail' });
    expect(gmail).toBeEnabled();
    expect(screen.getByText('Gmail')).toBeInTheDocument();
    expect(
      screen.getByText('Connect your email for smart replies and summaries')
    ).toBeInTheDocument();

    fireEvent.click(gmail);
    expect(onAdvance).toHaveBeenCalledWith('gmail');
  });

  it('disables completed action cards and exposes completed aria copy', () => {
    renderWithProvider(connectState, <WalkthroughActionCard step={connectState.steps[1]} />);

    expect(screen.getByRole('button', { name: /Slack.*completed/ })).toBeDisabled();
  });

  it('renders progress state for completed, current, and future phases', () => {
    renderWithProvider(connectState, <WalkthroughProgressBar />);

    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '2');
    expect(screen.getByLabelText('Welcome (completed)')).toBeInTheDocument();
    expect(screen.getByLabelText('Connect (current)')).toBeInTheDocument();
    expect(screen.getByLabelText('Automate')).toBeInTheDocument();
  });

  it('renders active phase copy, action cards, and skip control', () => {
    const onSkip = vi.fn();
    renderWithProvider(connectState, <WalkthroughPhasePanel />, { onSkip });

    expect(screen.getByText('Connect')).toBeInTheDocument();
    expect(
      screen.getByText(
        'Connect the tools you already use. Each connection gives your assistant new abilities.'
      )
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Skip this step' }));
    expect(onSkip).toHaveBeenCalledTimes(1);
  });

  it('renders the review empty and skipped states', () => {
    const reviewState: WalkthroughState = {
      phase: 'review',
      steps: [],
      completed: false,
      skipped: false,
    };

    const { rerender } = render(
      <WalkthroughProvider
        state={reviewState}
        onAdvance={vi.fn(() => reviewState)}
        onSkip={vi.fn()}>
        <WalkthroughPhasePanel />
      </WalkthroughProvider>
    );

    expect(screen.getByText('No actions completed yet.')).toBeInTheDocument();

    const skippedState = { ...reviewState, skipped: true };
    rerender(
      <WalkthroughProvider
        state={skippedState}
        onAdvance={vi.fn(() => skippedState)}
        onSkip={vi.fn()}>
        <WalkthroughPhasePanel />
      </WalkthroughProvider>
    );

    expect(
      screen.getByText('You skipped the setup. You can configure these anytime in Settings.')
    ).toBeInTheDocument();
  });

  it('renders the done state without action cards', () => {
    const doneState: WalkthroughState = {
      phase: 'done',
      steps: [],
      completed: true,
      skipped: false,
    };

    renderWithProvider(doneState, <WalkthroughPhasePanel />);

    expect(screen.getByText("You're all set!")).toBeInTheDocument();
    expect(
      screen.getByText(
        'Your assistant is ready to help. Connections are set up and automations are configured.'
      )
    ).toBeInTheDocument();
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('throws when the UI hook is used outside its provider', () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    expect(() => render(<WalkthroughProgressBar />)).toThrow(
      'useWalkthroughUI must be used within a WalkthroughProvider'
    );

    errorSpy.mockRestore();
  });
});

vi.mock('../../../pages/onboarding/OnboardingContext', () => ({ useOnboardingContext: vi.fn() }));

describe('WalkthroughContainer', () => {
  it('renders nothing when onboarding draft has no walkthrough', async () => {
    const { useOnboardingContext } = await import('../../../pages/onboarding/OnboardingContext');
    vi.mocked(useOnboardingContext).mockReturnValue({
      draft: { connectedSources: [] },
      setDraft: vi.fn(),
      completeAndExit: vi.fn(),
      advanceWalkthrough: vi.fn(),
      skipWalkthrough: vi.fn(),
    });

    const { container } = render(<WalkthroughContainer />);
    expect(container.firstChild).toBeNull();
  });

  it('wires onboarding context actions into the phase panel', async () => {
    const { useOnboardingContext } = await import('../../../pages/onboarding/OnboardingContext');
    const advanceWalkthrough = vi.fn(() => connectState);
    const skipWalkthrough = vi.fn();
    vi.mocked(useOnboardingContext).mockReturnValue({
      draft: { connectedSources: [], walkthrough: connectState },
      setDraft: vi.fn(),
      completeAndExit: vi.fn(),
      advanceWalkthrough,
      skipWalkthrough,
    });

    render(<WalkthroughContainer />);

    fireEvent.click(screen.getByRole('button', { name: 'Complete Gmail' }));
    fireEvent.click(screen.getByRole('button', { name: 'Skip this step' }));

    expect(advanceWalkthrough).toHaveBeenCalledWith('gmail');
    expect(skipWalkthrough).toHaveBeenCalledTimes(1);
  });
});
