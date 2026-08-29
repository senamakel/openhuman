import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import OrchestratorTaskBoard from './OrchestratorTaskBoard';

// Pass-through translator so assertions can target the i18n keys directly.
vi.mock('../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (key: string) => key }) }));

const EMPTY = { threadId: 'orchestrator-tasks', cards: [], updatedAt: '' };
const list = vi.fn(async () => EMPTY);
const add = vi.fn(async () => EMPTY);

vi.mock('../../services/api/todosApi', () => ({
  ORCHESTRATOR_TASKS_THREAD_ID: 'orchestrator-tasks',
  todosApi: {
    list: (...args: unknown[]) => list(...(args as [])),
    add: (...args: unknown[]) => add(...(args as [])),
    edit: vi.fn(),
    remove: vi.fn(),
    updateStatus: vi.fn(),
  },
}));

vi.mock('../../features/conversations/components/TaskKanbanBoard', () => ({
  TaskKanbanBoard: () => <div data-testid="kanban" />,
}));

describe('OrchestratorTaskBoard', () => {
  it('adds a task through the migrated TextField composer', async () => {
    render(<OrchestratorTaskBoard />);
    await waitFor(() => expect(list).toHaveBeenCalled());

    const input = screen.getByTestId('orch-task-add-input');
    const submit = screen.getByTestId('orch-task-add-submit');

    // Empty draft keeps the submit disabled.
    expect(submit).toBeDisabled();

    fireEvent.change(input, { target: { value: 'Ship the rail' } });
    expect(submit).not.toBeDisabled();

    fireEvent.click(submit);
    await waitFor(() =>
      expect(add).toHaveBeenCalledWith({ threadId: 'orchestrator-tasks', content: 'Ship the rail' })
    );
    await waitFor(() => expect((input as HTMLInputElement).value).toBe(''));
  });
});
