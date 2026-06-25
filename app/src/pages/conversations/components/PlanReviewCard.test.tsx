import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { TaskBoardCard } from '../../../types/turnState';
import { PlanReviewCard } from './PlanReviewCard';

// Echo i18n keys so we can assert on the stable key string.
vi.mock('../../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (key: string) => key }) }));

function card(partial: Partial<TaskBoardCard>): TaskBoardCard {
  return {
    id: 'c1',
    title: 'Do thing',
    status: 'awaiting_approval',
    order: 0,
    updatedAt: '',
    ...partial,
  } as TaskBoardCard;
}

const noop = () => {};

describe('PlanReviewCard', () => {
  it('renders nothing when there are no awaiting cards', () => {
    const { container } = render(
      <PlanReviewCard cards={[]} onApprove={noop} onReject={noop} onSendFeedback={noop} />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('renders each card title and its plan steps in order', () => {
    render(
      <PlanReviewCard
        cards={[
          card({ id: 'b', title: 'Second', order: 1 }),
          card({ id: 'a', title: 'First', order: 0, plan: ['step one', 'step two'] }),
        ]}
        onApprove={noop}
        onReject={noop}
        onSendFeedback={noop}
      />
    );
    expect(screen.getByText('First')).toBeInTheDocument();
    expect(screen.getByText('Second')).toBeInTheDocument();
    expect(screen.getByText('step one')).toBeInTheDocument();
    expect(screen.getByText('step two')).toBeInTheDocument();
  });

  it('fires approve and reject', () => {
    const onApprove = vi.fn();
    const onReject = vi.fn();
    render(
      <PlanReviewCard
        cards={[card({})]}
        onApprove={onApprove}
        onReject={onReject}
        onSendFeedback={noop}
      />
    );
    fireEvent.click(screen.getByText('conversations.planReview.approve'));
    fireEvent.click(screen.getByText('conversations.planReview.reject'));
    expect(onApprove).toHaveBeenCalledTimes(1);
    expect(onReject).toHaveBeenCalledTimes(1);
  });

  it('sends trimmed feedback and clears the textarea, ignoring blank input', () => {
    const onSendFeedback = vi.fn();
    render(
      <PlanReviewCard
        cards={[card({})]}
        onApprove={noop}
        onReject={noop}
        onSendFeedback={onSendFeedback}
      />
    );
    const send = screen.getByText('conversations.planReview.sendFeedback');
    const textarea = screen.getByTestId('plan-review-feedback') as HTMLTextAreaElement;

    // Blank → disabled, no call.
    fireEvent.click(send);
    expect(onSendFeedback).not.toHaveBeenCalled();

    fireEvent.change(textarea, { target: { value: '  add a verification step  ' } });
    fireEvent.click(send);
    expect(onSendFeedback).toHaveBeenCalledWith('add a verification step');
    // Textarea resets after sending.
    expect(textarea.value).toBe('');
  });

  it('disables every control when disabled', () => {
    render(
      <PlanReviewCard
        cards={[card({})]}
        onApprove={noop}
        onReject={noop}
        onSendFeedback={noop}
        disabled
      />
    );
    expect(screen.getByText('conversations.planReview.approve')).toBeDisabled();
    expect(screen.getByText('conversations.planReview.reject')).toBeDisabled();
    expect(screen.getByTestId('plan-review-feedback')).toBeDisabled();
  });
});
