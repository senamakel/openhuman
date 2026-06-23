import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { TaskBoard, TaskBoardCard } from '../../../types/turnState';
import { ThreadTodoStrip } from './ThreadTodoStrip';

// Echo i18n keys so we can assert on the stable key string.
vi.mock('../../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (key: string) => key }) }));

function card(partial: Partial<TaskBoardCard>): TaskBoardCard {
  return {
    id: 'c1',
    title: 'Do thing',
    status: 'todo',
    order: 0,
    updatedAt: '',
    ...partial,
  } as TaskBoardCard;
}

function board(cards: TaskBoardCard[]): TaskBoard {
  return { threadId: 't1', cards, updatedAt: '' };
}

describe('ThreadTodoStrip', () => {
  it('renders nothing when board is null', () => {
    const { container } = render(<ThreadTodoStrip board={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('renders nothing when there are no active cards', () => {
    const { container } = render(
      <ThreadTodoStrip
        board={board([card({ id: 'a', status: 'done' }), card({ id: 'b', status: 'rejected' })])}
      />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('lists active cards and hides done/rejected ones', () => {
    render(
      <ThreadTodoStrip
        board={board([
          card({ id: 'a', title: 'Active work', status: 'in_progress' }),
          card({ id: 'b', title: 'Queued work', status: 'todo' }),
          card({ id: 'c', title: 'Finished work', status: 'done' }),
          card({ id: 'd', title: 'Dropped work', status: 'rejected' }),
        ])}
      />
    );
    expect(screen.getByText('Active work')).toBeInTheDocument();
    expect(screen.getByText('Queued work')).toBeInTheDocument();
    expect(screen.queryByText('Finished work')).not.toBeInTheDocument();
    expect(screen.queryByText('Dropped work')).not.toBeInTheDocument();
  });

  it('shows a done/total progress count excluding rejected cards', () => {
    render(
      <ThreadTodoStrip
        board={board([
          card({ id: 'a', status: 'in_progress' }),
          card({ id: 'b', status: 'done' }),
          card({ id: 'c', status: 'rejected' }),
        ])}
      />
    );
    // 1 done out of 2 tracked (rejected excluded entirely).
    expect(screen.getByText('1/2')).toBeInTheDocument();
  });

  it('orders active cards by their `order` field', () => {
    render(
      <ThreadTodoStrip
        board={board([
          card({ id: 'a', title: 'Second', status: 'todo', order: 5 }),
          card({ id: 'b', title: 'First', status: 'todo', order: 1 }),
        ])}
      />
    );
    const items = screen.getAllByRole('listitem').map(li => li.textContent);
    expect(items[0]).toContain('First');
    expect(items[1]).toContain('Second');
  });

  it('falls back to objective then id when title is blank', () => {
    render(
      <ThreadTodoStrip
        board={board([
          card({ id: 'a', title: '   ', objective: 'Ship it', status: 'todo' }),
          card({ id: 'only-id', title: '', objective: null, status: 'todo' }),
        ])}
      />
    );
    expect(screen.getByText('Ship it')).toBeInTheDocument();
    expect(screen.getByText('only-id')).toBeInTheDocument();
  });

  it('collapses and expands the list when the header is clicked', () => {
    render(<ThreadTodoStrip board={board([card({ id: 'a', title: 'Active work', status: 'todo' })])} />);
    expect(screen.getByText('Active work')).toBeInTheDocument();
    const header = screen.getByRole('button', { expanded: true });
    fireEvent.click(header);
    expect(screen.queryByText('Active work')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { expanded: false }));
    expect(screen.getByText('Active work')).toBeInTheDocument();
  });
});
