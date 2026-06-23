import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { goalsApi } from '../../services/api/goalsApi';
import GoalsPanel from './GoalsPanel';

vi.mock('../../services/api/goalsApi', () => ({
  goalsApi: { list: vi.fn(), add: vi.fn(), edit: vi.fn(), remove: vi.fn(), reflect: vi.fn() },
}));

const api = vi.mocked(goalsApi);

describe('<GoalsPanel />', () => {
  beforeEach(() => vi.clearAllMocks());

  it('renders the empty state once loading resolves', async () => {
    api.list.mockResolvedValueOnce([]);
    render(<GoalsPanel />);
    expect(await screen.findByText(/No goals yet/)).toBeInTheDocument();
    expect(screen.getByText('Long-term Goals')).toBeInTheDocument();
  });

  it('renders loaded goals', async () => {
    api.list.mockResolvedValueOnce([
      { id: 'g1', text: 'Ship the desktop app' },
      { id: 'g2', text: 'Keep the Rust core authoritative' },
    ]);
    render(<GoalsPanel />);
    expect(await screen.findByText('Ship the desktop app')).toBeInTheDocument();
    expect(screen.getByText('Keep the Rust core authoritative')).toBeInTheDocument();
  });

  it('adds a goal from the input and renders the updated list', async () => {
    api.list.mockResolvedValueOnce([]);
    api.add.mockResolvedValueOnce([{ id: 'g1', text: 'New goal' }]);
    render(<GoalsPanel />);
    await screen.findByText(/No goals yet/);

    fireEvent.change(screen.getByPlaceholderText('Add a long-term goal…'), {
      target: { value: 'New goal' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Add/ }));

    await waitFor(() => expect(api.add).toHaveBeenCalledWith('New goal'));
    expect(await screen.findByText('New goal')).toBeInTheDocument();
  });

  it('deletes a goal', async () => {
    api.list.mockResolvedValueOnce([{ id: 'g1', text: 'Doomed goal' }]);
    api.remove.mockResolvedValueOnce([]);
    render(<GoalsPanel />);
    await screen.findByText('Doomed goal');

    fireEvent.click(screen.getByRole('button', { name: 'Delete goal' }));
    await waitFor(() => expect(api.remove).toHaveBeenCalledWith('g1'));
  });

  it('runs reflect and shows the agent summary', async () => {
    api.list.mockResolvedValueOnce([]);
    api.reflect.mockResolvedValueOnce({
      ran: true,
      summary: 'Added 2 goals from context',
      items: [
        { id: 'g1', text: 'A' },
        { id: 'g2', text: 'B' },
      ],
    });
    render(<GoalsPanel />);
    await screen.findByText(/No goals yet/);

    fireEvent.click(screen.getByRole('button', { name: /Reflect/ }));
    await waitFor(() => expect(api.reflect).toHaveBeenCalled());
    expect(await screen.findByText('Added 2 goals from context')).toBeInTheDocument();
    expect(screen.getByText('A')).toBeInTheDocument();
  });

  it('surfaces an action error when add fails', async () => {
    api.list.mockResolvedValueOnce([]);
    api.add.mockRejectedValueOnce(new Error('boom'));
    render(<GoalsPanel />);
    await screen.findByText(/No goals yet/);

    fireEvent.change(screen.getByPlaceholderText('Add a long-term goal…'), {
      target: { value: 'x' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Add/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent('boom');
  });
});
