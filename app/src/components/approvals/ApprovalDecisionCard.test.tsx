import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import ApprovalDecisionCard, { type ApprovalDecisionAction } from './ApprovalDecisionCard';

const actions: ApprovalDecisionAction[] = [
  { id: 'approve', label: 'Approve once', busyLabel: 'Approving…', variant: 'primary' },
  {
    id: 'always',
    label: 'Approve always',
    busyLabel: 'Saving…',
    variant: 'secondary',
    title: 'Allow this flow in future',
  },
  { id: 'deny', label: 'Deny', busyLabel: 'Denying…', variant: 'secondary', tone: 'danger' },
];

describe('ApprovalDecisionCard', () => {
  it('composes an alert dialog from its summary and optional metadata', () => {
    render(
      <ApprovalDecisionCard
        ariaLabel="Workflow approval"
        summary={<span>Run the shell command</span>}
        metadata={<span>Tool: shell</span>}
        actions={actions}
        onAction={vi.fn()}
        testId="approval-card"
        className="text-sm"
      />
    );

    const card = screen.getByRole('alertdialog', { name: 'Workflow approval' });
    expect(card).toHaveAttribute('data-testid', 'approval-card');
    expect(card).toHaveClass('text-sm');
    expect(within(card).getByText('Run the shell command')).toBeInTheDocument();
    expect(within(card).getByText('Tool: shell')).toBeInTheDocument();
  });

  it('renders actions in descriptor order and passes the selected action id', () => {
    const onAction = vi.fn();
    render(
      <ApprovalDecisionCard
        ariaLabel="Workflow approval"
        summary="Run the shell command"
        actions={actions}
        onAction={onAction}
      />
    );

    expect(screen.getAllByRole('button').map(button => button.textContent)).toEqual([
      'Approve once',
      'Approve always',
      'Deny',
    ]);

    fireEvent.click(screen.getByRole('button', { name: 'Approve always' }));
    expect(onAction).toHaveBeenCalledWith('always');
  });

  it('shows the busy label only for the active action and disables every action', () => {
    render(
      <ApprovalDecisionCard
        ariaLabel="Workflow approval"
        summary="Run the shell command"
        actions={actions}
        busyActionId="always"
        onAction={vi.fn()}
      />
    );

    expect(screen.getByRole('button', { name: 'Approve once' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Saving…' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Deny' })).toBeDisabled();
    expect(screen.queryByText('Approving…')).not.toBeInTheDocument();
    expect(screen.queryByText('Denying…')).not.toBeInTheDocument();
  });

  it('forwards variant, tone, title, and action ids to the rendered buttons', () => {
    render(
      <ApprovalDecisionCard
        ariaLabel="Workflow approval"
        summary="Run the shell command"
        actions={actions}
        onAction={vi.fn()}
      />
    );

    const approve = screen.getByRole('button', { name: 'Approve once' });
    const always = screen.getByRole('button', { name: 'Approve always' });
    const deny = screen.getByRole('button', { name: 'Deny' });

    expect(approve).toHaveAttribute('data-testid', 'approve');
    expect(approve).toHaveClass('bg-primary-500');
    expect(always).toHaveAttribute('data-testid', 'always');
    expect(always).toHaveClass('border-line-strong');
    expect(always).toHaveAttribute('title', 'Allow this flow in future');
    expect(deny).toHaveAttribute('data-testid', 'deny');
    expect(deny).toHaveClass('text-coral-600');
    expect(deny).toHaveClass('border-coral-300/50');
  });
});
