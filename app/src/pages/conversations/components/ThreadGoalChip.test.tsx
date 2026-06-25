import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { ThreadGoal } from '../../../services/api/threadGoalApi';
import { ThreadGoalChip } from './ThreadGoalChip';

// Echo i18n keys so assertions are on stable key strings.
vi.mock('../../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (key: string) => key }) }));

function goal(partial: Partial<ThreadGoal> = {}): ThreadGoal {
  return {
    threadId: 't1',
    goalId: 'g1',
    objective: 'Ship the release',
    status: 'active',
    tokenBudget: 1000,
    tokensUsed: 250,
    timeUsedSeconds: 0,
    createdAtMs: 0,
    updatedAtMs: 0,
    continuationSuppressed: false,
    ...partial,
  };
}

function stubApi(initial: ThreadGoal | null) {
  return {
    get: vi.fn().mockResolvedValue(initial),
    set: vi.fn().mockResolvedValue(goal()),
    complete: vi.fn().mockResolvedValue(goal({ status: 'complete' })),
    pause: vi.fn().mockResolvedValue(goal({ status: 'paused' })),
    resume: vi.fn().mockResolvedValue(goal()),
    clear: vi.fn().mockResolvedValue(true),
  } as unknown as typeof import('../../../services/api/threadGoalApi').threadGoalApi;
}

describe('ThreadGoalChip', () => {
  it('shows a Set goal affordance when the thread has no goal', async () => {
    const api = stubApi(null);
    render(<ThreadGoalChip threadId="t1" api={api} />);
    await waitFor(() => expect(api.get).toHaveBeenCalledWith('t1'));
    expect(await screen.findByText('conversations.threadGoal.setCta')).toBeTruthy();
  });

  it('renders the objective, status, and token budget when a goal exists', async () => {
    const api = stubApi(goal());
    render(<ThreadGoalChip threadId="t1" api={api} />);
    expect(await screen.findByText('Ship the release')).toBeTruthy();
    expect(screen.getByText('conversations.threadGoal.status.active')).toBeTruthy();
    // Budget formatted "250 / 1,000 <tokensSuffix>".
    expect(screen.getByText(/250 \/ 1,000/)).toBeTruthy();
  });

  it('sets a goal from the edit form', async () => {
    const api = stubApi(null);
    render(<ThreadGoalChip threadId="t1" api={api} />);
    fireEvent.click(await screen.findByText('conversations.threadGoal.setCta'));
    const input = screen.getByPlaceholderText('conversations.threadGoal.placeholder');
    fireEvent.change(input, { target: { value: 'New objective' } });
    fireEvent.click(screen.getByText('conversations.threadGoal.save'));
    await waitFor(() => expect(api.set).toHaveBeenCalledWith('t1', 'New objective'));
  });

  it('completes a goal via the action button', async () => {
    const api = stubApi(goal());
    render(<ThreadGoalChip threadId="t1" api={api} />);
    await screen.findByText('Ship the release');
    fireEvent.click(screen.getByLabelText('conversations.threadGoal.complete'));
    await waitFor(() => expect(api.complete).toHaveBeenCalledWith('t1'));
  });

  it('shows Resume (not Pause) for a paused goal', async () => {
    const api = stubApi(goal({ status: 'paused' }));
    render(<ThreadGoalChip threadId="t1" api={api} />);
    await screen.findByText('Ship the release');
    expect(screen.getByLabelText('conversations.threadGoal.resume')).toBeTruthy();
    expect(screen.queryByLabelText('conversations.threadGoal.pause')).toBeNull();
  });
});
