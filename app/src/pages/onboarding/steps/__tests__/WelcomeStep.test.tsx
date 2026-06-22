import { fireEvent, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { renderWithProviders } from '../../../../test/test-utils';
import WelcomeStep from '../WelcomeStep';

describe('WelcomeStep', () => {
  it('renders the hero title', () => {
    renderWithProviders(<WelcomeStep onNext={() => {}} />);
    expect(
      screen.getByRole('heading', { level: 1, name: /AI hub for work and life/i })
    ).toBeInTheDocument();
  });

  it('renders the flow steps: Connect, Automate, Deliver', () => {
    renderWithProviders(<WelcomeStep onNext={() => {}} />);
    expect(screen.getByText('Connect')).toBeInTheDocument();
    expect(screen.getByText('Automate')).toBeInTheDocument();
    expect(screen.getByText('Deliver')).toBeInTheDocument();
  });

  it('renders capability cards', () => {
    renderWithProviders(<WelcomeStep onNext={() => {}} />);
    expect(screen.getByText('Manage work')).toBeInTheDocument();
    expect(screen.getByText('Automate everything')).toBeInTheDocument();
    expect(screen.getByText('Stay private')).toBeInTheDocument();
  });

  it('exposes a "What leaves my computer?" link', () => {
    renderWithProviders(<WelcomeStep onNext={() => {}} />);
    expect(screen.getByRole('button', { name: 'What leaves my computer?' })).toBeInTheDocument();
  });

  it('fires onNext when the CTA is clicked', () => {
    const onNext = vi.fn();
    renderWithProviders(<WelcomeStep onNext={onNext} />);
    fireEvent.click(screen.getByRole('button', { name: 'Get Started' }));
    expect(onNext).toHaveBeenCalledTimes(1);
  });

  it('CTA is always enabled (WelcomeStep has no disabled/loading props)', () => {
    renderWithProviders(<WelcomeStep onNext={() => {}} />);
    expect(screen.getByRole('button', { name: 'Get Started' })).not.toBeDisabled();
  });

  it('CTA has proper ARIA attributes for accessibility', () => {
    renderWithProviders(<WelcomeStep onNext={() => {}} />);
    const button = screen.getByRole('button', { name: 'Get Started' });

    expect(button).toHaveAttribute('aria-label', 'Get Started');
    expect(button).toHaveAttribute('aria-live', 'polite');
    expect(button).toHaveAttribute('aria-busy', 'false');
  });

  it('renders the logo image', () => {
    renderWithProviders(<WelcomeStep onNext={() => {}} />);
    expect(screen.getByAltText('OpenHuman')).toBeInTheDocument();
  });

  it('renders the hero subtitle', () => {
    renderWithProviders(<WelcomeStep onNext={() => {}} />);
    expect(screen.getByText(/connects your tools, automates your tasks/i)).toBeInTheDocument();
  });

  it('renders the flow step descriptions', () => {
    renderWithProviders(<WelcomeStep onNext={() => {}} />);
    expect(screen.getByText(/Link your tools, accounts, and services/i)).toBeInTheDocument();
    expect(
      screen.getByText(/Your assistant works across your connected world/i)
    ).toBeInTheDocument();
    expect(screen.getByText(/Get summaries, completed tasks, and insights/i)).toBeInTheDocument();
  });
});
