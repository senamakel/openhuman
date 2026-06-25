import React, { useState } from 'react';

import Button from '../../../components/ui/Button';
import { useT } from '../../../lib/i18n/I18nContext';
import type { TaskBoardCard } from '../../../types/turnState';

/**
 * Plan-mode review surface (Codex/Claude-style). When an interactive turn lays
 * out a thread-scoped plan, the orchestrator parks the to-do cards at
 * `awaiting_approval` and stops before executing. This card aggregates the
 * whole parked plan (objective + ordered steps per card) into a single
 * "review once" panel above the composer and offers three actions:
 *
 *  - **Approve & run** → every awaiting card becomes runnable (the dispatcher
 *    picks them up).
 *  - **Reject** → the plan is dropped; nothing executes.
 *  - **Send feedback** → the parked plan is cleared and the free-text request
 *    is sent back into the thread so the agent re-plans (and re-parks).
 *
 * Pure presentation: the page owns the RPCs (see `taskPlanActions`). Mirrors
 * {@link ApprovalRequestCard}'s placement/styling so the two parked-turn
 * surfaces read consistently.
 */

interface Props {
  /** The cards currently awaiting plan approval for the active thread. */
  cards: TaskBoardCard[];
  onApprove: () => void;
  onReject: () => void;
  onSendFeedback: (feedback: string) => void;
  /** Disable all controls (e.g. no thread selected, or a decision in flight). */
  disabled?: boolean;
}

/** Prefer the card title; fall back to its objective, then its id. */
function cardLabel(card: TaskBoardCard): string {
  return card.title?.trim() || card.objective?.trim() || card.id;
}

export const PlanReviewCard: React.FC<Props> = ({
  cards,
  onApprove,
  onReject,
  onSendFeedback,
  disabled = false,
}) => {
  const { t } = useT();
  const [feedback, setFeedback] = useState('');

  if (cards.length === 0) return null;

  const submitFeedback = () => {
    const trimmed = feedback.trim();
    if (!trimmed) return;
    onSendFeedback(trimmed);
    setFeedback('');
  };

  return (
    <div
      role="alertdialog"
      aria-label={t('conversations.planReview.title')}
      data-testid="plan-review-card"
      className="mb-2 rounded-xl border border-ocean-300 bg-ocean-50 p-3 text-sm shadow-sm dark:border-ocean-700 dark:bg-ocean-950">
      <div className="flex items-start gap-2">
        <span aria-hidden className="text-base leading-none text-ocean-700 dark:text-ocean-200">
          🗺️
        </span>
        <div className="min-w-0 flex-1">
          <p className="font-semibold text-ocean-900 dark:text-ocean-100">
            {t('conversations.planReview.title')}
          </p>
          <p className="mt-1 text-ocean-800/90 dark:text-ocean-200/90">
            {t('conversations.planReview.subtitle')}
          </p>

          <ul className="mt-2 flex max-h-56 flex-col gap-2 overflow-y-auto">
            {cards
              .slice()
              .sort((a, b) => a.order - b.order)
              .map(card => (
                <li
                  key={card.id}
                  className="rounded-lg border border-ocean-200/80 bg-white px-2.5 py-2 dark:border-ocean-800 dark:bg-neutral-950">
                  <p className="font-medium text-ink dark:text-neutral-100">{cardLabel(card)}</p>
                  {card.objective?.trim() && card.objective.trim() !== cardLabel(card) && (
                    <p className="mt-0.5 text-xs text-stone-600 dark:text-neutral-300">
                      {t('conversations.planReview.objective')}: {card.objective.trim()}
                    </p>
                  )}
                  {(card.plan?.length ?? 0) > 0 && (
                    <ol className="mt-1 list-decimal pl-5 text-xs text-stone-600 dark:text-neutral-300">
                      {card.plan?.map((step, i) => (
                        <li key={i} className="break-words">
                          {step}
                        </li>
                      ))}
                    </ol>
                  )}
                </li>
              ))}
          </ul>

          <div className="mt-3 flex flex-wrap items-center gap-2">
            <Button
              variant="primary"
              size="sm"
              data-analytics-id="plan-review-approve"
              onClick={onApprove}
              disabled={disabled}>
              {t('conversations.planReview.approve')}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              data-analytics-id="plan-review-reject"
              onClick={onReject}
              disabled={disabled}>
              {t('conversations.planReview.reject')}
            </Button>
          </div>

          <div className="mt-3">
            <label
              htmlFor="plan-review-feedback"
              className="mb-1 block text-xs font-medium text-ocean-800/80 dark:text-ocean-200/80">
              {t('conversations.planReview.feedbackLabel')}
            </label>
            <textarea
              id="plan-review-feedback"
              data-testid="plan-review-feedback"
              value={feedback}
              onChange={e => setFeedback(e.target.value)}
              onKeyDown={e => {
                // Cmd/Ctrl+Enter submits, matching the composer's quick-send.
                if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                  e.preventDefault();
                  submitFeedback();
                }
              }}
              rows={2}
              disabled={disabled}
              placeholder={t('conversations.planReview.feedbackPlaceholder')}
              className="w-full resize-y rounded-lg border border-ocean-200 bg-white px-2.5 py-1.5 text-sm text-ink shadow-inner outline-none focus:border-ocean-400 disabled:opacity-50 dark:border-ocean-800 dark:bg-neutral-950 dark:text-neutral-100"
            />
            <div className="mt-1.5 flex justify-end">
              <Button
                variant="secondary"
                size="sm"
                data-analytics-id="plan-review-send-feedback"
                onClick={submitFeedback}
                disabled={disabled || feedback.trim().length === 0}>
                {t('conversations.planReview.sendFeedback')}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default PlanReviewCard;
