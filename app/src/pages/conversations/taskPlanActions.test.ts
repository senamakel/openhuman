import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { TaskBoardCard } from '../../types/turnState';
import { runDecidePlan, runDecidePlanAll, runRevisePlan } from './taskPlanActions';

const mockDecidePlan = vi.fn();
const mockRevisePlan = vi.fn();
vi.mock('../../services/api/threadApi', () => ({
  threadApi: {
    decidePlan: (...args: unknown[]) => mockDecidePlan(...args),
    revisePlan: (...args: unknown[]) => mockRevisePlan(...args),
  },
}));

function card(id = 'c1'): TaskBoardCard {
  return { id, title: 'T', status: 'awaiting_approval', order: 0, updatedAt: '' };
}

const t = (key: string) => key;

describe('runDecidePlan', () => {
  beforeEach(() => mockDecidePlan.mockReset());

  it('is a no-op when there is no active thread', async () => {
    const dispatch = vi.fn();
    const notify = vi.fn();

    await runDecidePlan({ threadId: null, card: card(), approve: true, dispatch, notify, t });

    expect(mockDecidePlan).not.toHaveBeenCalled();
    expect(dispatch).not.toHaveBeenCalled();
    expect(notify).not.toHaveBeenCalled();
  });

  it('dispatches the refreshed board on success', async () => {
    mockDecidePlan.mockResolvedValueOnce({ threadId: 't1', cards: [], updatedAt: '' });
    const dispatch = vi.fn();
    const notify = vi.fn();

    await runDecidePlan({ threadId: 't1', card: card(), approve: true, dispatch, notify, t });

    expect(mockDecidePlan).toHaveBeenCalledWith('t1', 'c1', true);
    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(notify).not.toHaveBeenCalled();
  });

  it('does not dispatch when the RPC returns null', async () => {
    mockDecidePlan.mockResolvedValueOnce(null);
    const dispatch = vi.fn();
    const notify = vi.fn();

    await runDecidePlan({ threadId: 't1', card: card(), approve: false, dispatch, notify, t });

    expect(mockDecidePlan).toHaveBeenCalledWith('t1', 'c1', false);
    expect(dispatch).not.toHaveBeenCalled();
    expect(notify).not.toHaveBeenCalled();
  });

  it('notifies (without throwing or dispatching) on RPC failure', async () => {
    mockDecidePlan.mockRejectedValueOnce(new Error('boom'));
    const dispatch = vi.fn();
    const notify = vi.fn();

    await runDecidePlan({ threadId: 't1', card: card(), approve: true, dispatch, notify, t });

    expect(notify).toHaveBeenCalledWith('conversations.taskKanban.updateFailed');
    expect(dispatch).not.toHaveBeenCalled();
  });
});

describe('runDecidePlanAll', () => {
  beforeEach(() => mockDecidePlan.mockReset());

  it('is a no-op with no thread or no cards', async () => {
    const dispatch = vi.fn();
    const notify = vi.fn();
    await runDecidePlanAll({ threadId: null, cards: [card()], approve: true, dispatch, notify, t });
    await runDecidePlanAll({ threadId: 't1', cards: [], approve: true, dispatch, notify, t });
    expect(mockDecidePlan).not.toHaveBeenCalled();
    expect(dispatch).not.toHaveBeenCalled();
  });

  it('decides every awaiting card and dispatches the last board', async () => {
    mockDecidePlan
      .mockResolvedValueOnce({ threadId: 't1', cards: [], updatedAt: '' })
      .mockResolvedValueOnce({ threadId: 't1', cards: [], updatedAt: 'final' });
    const dispatch = vi.fn();
    const notify = vi.fn();

    await runDecidePlanAll({
      threadId: 't1',
      cards: [card('c1'), card('c2')],
      approve: true,
      dispatch,
      notify,
      t,
    });

    expect(mockDecidePlan).toHaveBeenNthCalledWith(1, 't1', 'c1', true);
    expect(mockDecidePlan).toHaveBeenNthCalledWith(2, 't1', 'c2', true);
    // Only the final snapshot is dispatched.
    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(notify).not.toHaveBeenCalled();
  });

  it('notifies (without throwing) on RPC failure', async () => {
    mockDecidePlan.mockRejectedValueOnce(new Error('boom'));
    const dispatch = vi.fn();
    const notify = vi.fn();
    await runDecidePlanAll({
      threadId: 't1',
      cards: [card()],
      approve: false,
      dispatch,
      notify,
      t,
    });
    expect(notify).toHaveBeenCalledWith('conversations.taskKanban.updateFailed');
  });
});

describe('runRevisePlan', () => {
  beforeEach(() => {
    mockRevisePlan.mockReset();
  });

  it('is a no-op with no thread or blank feedback', async () => {
    const dispatch = vi.fn();
    const notify = vi.fn();
    const sendMessage = vi.fn();
    await runRevisePlan({ threadId: null, feedback: 'x', sendMessage, dispatch, notify, t });
    await runRevisePlan({ threadId: 't1', feedback: '   ', sendMessage, dispatch, notify, t });
    expect(mockRevisePlan).not.toHaveBeenCalled();
    expect(sendMessage).not.toHaveBeenCalled();
  });

  it('clears the plan, dispatches the board, then sends the feedback message', async () => {
    mockRevisePlan.mockResolvedValueOnce({ threadId: 't1', cards: [], updatedAt: '' });
    const dispatch = vi.fn();
    const notify = vi.fn();
    const sendMessage = vi.fn();

    await runRevisePlan({
      threadId: 't1',
      feedback: '  add a test step  ',
      sendMessage,
      dispatch,
      notify,
      t,
    });

    // Trimmed before both the RPC and the message send.
    expect(mockRevisePlan).toHaveBeenCalledWith('t1', 'add a test step');
    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(sendMessage).toHaveBeenCalledWith('add a test step');
    expect(notify).not.toHaveBeenCalled();
  });

  it('notifies (without throwing) on RPC failure', async () => {
    mockRevisePlan.mockRejectedValueOnce(new Error('boom'));
    const dispatch = vi.fn();
    const notify = vi.fn();
    const sendMessage = vi.fn();
    await runRevisePlan({ threadId: 't1', feedback: 'redo', sendMessage, dispatch, notify, t });
    expect(notify).toHaveBeenCalledWith('conversations.taskKanban.updateFailed');
    expect(sendMessage).not.toHaveBeenCalled();
  });
});
