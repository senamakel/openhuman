/**
 * Task-plan approval action, extracted from `Conversations` so the whole
 * guard → decide → persist → refresh → error-advisory flow is unit-testable
 * without mounting the conversations page.
 */
import debugFactory from 'debug';

import { threadApi } from '../../services/api/threadApi';
import { setTaskBoardForThread } from '../../store/chatRuntimeSlice';
import type { TaskBoard, TaskBoardCard } from '../../types/turnState';

const debug = debugFactory('conversations:taskPlan');

export interface RunDecidePlanArgs {
  /** Active thread; a no-op when null (nothing to decide against). */
  threadId: string | null;
  card: TaskBoardCard;
  approve: boolean;
  /** Redux dispatch (kept loosely typed so callers don't need the store type). */
  dispatch: (action: unknown) => void;
  /** Surface a user-facing advisory on failure. */
  notify: (message: string) => void;
  /** Translator for the advisory message. */
  t: (key: string) => string;
}

/**
 * Approve or reject a card's plan via the core RPC, then optimistically refresh
 * the thread's board from the returned snapshot. A failure is logged and
 * surfaced via `notify` (never thrown) so a failed decision degrades
 * gracefully. No-op when `threadId` is null.
 */
export async function runDecidePlan({
  threadId,
  card,
  approve,
  dispatch,
  notify,
  t,
}: RunDecidePlanArgs): Promise<void> {
  if (!threadId) return;
  try {
    const saved = await threadApi.decidePlan(threadId, card.id, approve);
    if (saved) {
      dispatch(setTaskBoardForThread({ threadId, board: saved }));
    }
  } catch (error) {
    debug('decidePlan failed: %o', error);
    notify(t('conversations.taskKanban.updateFailed'));
  }
}

export interface RunDecidePlanAllArgs {
  /** Active thread; a no-op when null. */
  threadId: string | null;
  /** The cards awaiting approval to decide together (the whole parked plan). */
  cards: TaskBoardCard[];
  approve: boolean;
  dispatch: (action: unknown) => void;
  notify: (message: string) => void;
  t: (key: string) => string;
}

/**
 * Approve or reject an **entire** parked plan at once — the "review once"
 * surface decides every `awaiting_approval` card together. Decides each card
 * sequentially via the same per-card RPC, then refreshes the board from the
 * last snapshot. Graceful (never throws): a failure is logged and surfaced via
 * `notify`. No-op when `threadId` is null or there are no cards.
 */
export async function runDecidePlanAll({
  threadId,
  cards,
  approve,
  dispatch,
  notify,
  t,
}: RunDecidePlanAllArgs): Promise<void> {
  if (!threadId || cards.length === 0) return;
  try {
    let latest: TaskBoard | null = null;
    for (const card of cards) {
      latest = await threadApi.decidePlan(threadId, card.id, approve);
    }
    if (latest) {
      dispatch(setTaskBoardForThread({ threadId, board: latest }));
    }
  } catch (error) {
    debug('decidePlanAll failed: %o', error);
    notify(t('conversations.taskKanban.updateFailed'));
  }
}

export interface RunRevisePlanArgs {
  /** Active thread; a no-op when null. */
  threadId: string | null;
  /** The user's free-text revision request. Trimmed; a no-op when blank. */
  feedback: string;
  /**
   * Send the feedback back into the thread as a chat message so the
   * orchestrator re-plans. Wired by the page to its normal send path.
   */
  sendMessage: (text: string) => void | Promise<void>;
  dispatch: (action: unknown) => void;
  notify: (message: string) => void;
  t: (key: string) => string;
}

/**
 * "Send feedback" plan-review action: clear the parked plan (reject all
 * awaiting cards via `revise_plan`), refresh the board, then bounce the
 * feedback back into the thread as a chat message so the agent re-plans and
 * re-parks. Graceful: clears the plan first so the agent never re-plans against
 * a stale parked plan; a send failure still leaves the board cleared. No-op
 * when `threadId` is null or `feedback` is blank.
 */
export async function runRevisePlan({
  threadId,
  feedback,
  sendMessage,
  dispatch,
  notify,
  t,
}: RunRevisePlanArgs): Promise<void> {
  if (!threadId) return;
  const trimmed = feedback.trim();
  if (!trimmed) return;
  try {
    const board = await threadApi.revisePlan(threadId, trimmed);
    if (board) {
      dispatch(setTaskBoardForThread({ threadId, board }));
    }
    await sendMessage(trimmed);
  } catch (error) {
    debug('revisePlan failed: %o', error);
    notify(t('conversations.taskKanban.updateFailed'));
  }
}
