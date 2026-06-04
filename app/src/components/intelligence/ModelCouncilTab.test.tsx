import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ModelCouncilResult } from '../../services/api/modelCouncilApi';
import ModelCouncilTab from './ModelCouncilTab';

const mockRunCouncil = vi.fn();
const mockDispatch = vi.fn();

const mockState = {
  agentProfiles: {
    profiles: [
      {
        id: 'default',
        name: 'Default Agent',
        description: 'Default',
        agentId: 'openhuman.default',
        modelOverride: 'profile-model',
        builtIn: true,
      },
      {
        id: 'critic',
        name: 'Critic',
        description: 'Finds gaps',
        agentId: 'critic-agent',
        modelOverride: 'critic-model',
        builtIn: false,
      },
    ],
    activeProfileId: 'default',
    status: 'idle',
    error: null,
  },
};

vi.mock('../../services/api/modelCouncilApi', () => ({
  modelCouncilApi: { runCouncil: (...args: unknown[]) => mockRunCouncil(...args) },
}));

vi.mock('../../store/hooks', () => ({
  useAppDispatch: () => mockDispatch,
  useAppSelector: (selector: (state: typeof mockState) => unknown) => selector(mockState),
}));

vi.mock('../../features/human/Mascot', () => ({
  RiveMascot: ({ face }: { face?: string }) => <div data-testid="rive-mascot" data-face={face} />,
  getMascotPalette: () => ({ bodyFill: '#F7D145', neckShadowColor: '#B23C05' }),
  hexToArgbInt: () => 0xfff7d145,
}));

const RESULT: ModelCouncilResult = {
  question: 'What is the capital of France?',
  members: [
    { model: 'gpt-5.2', response: 'Paris is the capital.', error: null },
    { model: 'critic-model', response: null, error: 'rate limited' },
  ],
  chair_model: 'claude-opus-4-8',
  synthesis: 'Both that answered agree: Paris. One seat failed.',
};

const fillQuestion = () => {
  fireEvent.change(screen.getByLabelText('Question'), {
    target: { value: 'What is the capital of France?' },
  });
};

describe('ModelCouncilTab', () => {
  beforeEach(() => {
    mockRunCouncil.mockReset();
    mockDispatch.mockReset();
  });

  it('renders settings, shared reasoning, and three Rive council seats by default', () => {
    render(<ModelCouncilTab />);

    expect(screen.getByText('Model Council')).toBeInTheDocument();
    expect(screen.getByText('Council settings')).toBeInTheDocument();
    expect(screen.getByText('shared_reasoning.md')).toBeInTheDocument();
    expect(screen.getByLabelText('Shared reasoning file')).toHaveValue(
      [
        '# Shared reasoning',
        '- Claims the council agrees on:',
        '- Open disagreements:',
        '- Evidence or constraints to preserve:',
        '- Judge synthesis notes:',
      ].join('\n')
    );
    expect(screen.getAllByTestId('rive-mascot')).toHaveLength(3);
    expect(screen.getByText('Juror 1')).toBeInTheDocument();
    expect(screen.getByText('Juror 3')).toBeInTheDocument();
  });

  it('uses the jury count setting to resize the roster up to five', () => {
    render(<ModelCouncilTab />);

    fireEvent.click(screen.getByRole('button', { name: '5' }));

    expect(screen.getAllByTestId('rive-mascot')).toHaveLength(5);
    expect(screen.getAllByText('Juror 5')).toHaveLength(2);
    expect(screen.getByLabelText('Juror 5 name')).toBeInTheDocument();
  });

  it('disables Convene until a question is filled because seats and judge have defaults', () => {
    render(<ModelCouncilTab />);

    const run = screen.getByRole('button', { name: 'Convene council' });
    expect(run).toBeDisabled();
    fillQuestion();
    expect(run).not.toBeDisabled();
  });

  it('shows mascot deliberation and agent thoughts while the council is running', async () => {
    let resolveRun: (value: ModelCouncilResult) => void = () => {};
    mockRunCouncil.mockReturnValueOnce(
      new Promise<ModelCouncilResult>(resolve => {
        resolveRun = resolve;
      })
    );
    render(<ModelCouncilTab />);
    fillQuestion();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Convene council' }));
    });

    expect(screen.getByText('Council deliberation')).toBeInTheDocument();
    expect(screen.getAllByText('Thinking')).toHaveLength(3);
    expect(screen.getByText('Judge')).toBeInTheDocument();
    expect(
      screen.getByText(/Waiting for juror answers, then reading the shared reasoning file/)
    ).toBeInTheDocument();
    expect(screen.getAllByTestId('rive-mascot')).toHaveLength(7);
    expect(screen.getAllByTestId('rive-mascot')[0]).toHaveAttribute('data-face', 'thinking');

    await act(async () => {
      resolveRun(RESULT);
    });

    await waitFor(() => {
      expect(screen.queryByText('Council deliberation')).not.toBeInTheDocument();
    });
  });

  it('lets a council seat use a saved profile and submits that profile model', async () => {
    mockRunCouncil.mockResolvedValueOnce(RESULT);
    render(<ModelCouncilTab />);

    const firstSeat = screen.getByLabelText('Juror 1 name').closest('article');
    expect(firstSeat).not.toBeNull();
    fireEvent.click(within(firstSeat as HTMLElement).getByRole('tab', { name: 'Profile' }));
    fireEvent.change(screen.getByLabelText('Juror 1 profile'), { target: { value: 'critic' } });
    fillQuestion();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Convene council' }));
    });

    expect(mockRunCouncil).toHaveBeenCalledWith({
      question: expect.stringContaining('shared_reasoning.md'),
      member_models: ['critic-model', 'default', 'default'],
      chair_model: 'default',
    });
    expect(mockRunCouncil.mock.calls[0][0].question).toContain('User question:');
    expect(mockRunCouncil.mock.calls[0][0].question).toContain('What is the capital of France?');
  });

  it('lets the judge agent use a saved profile unless a model override is typed', async () => {
    mockRunCouncil.mockResolvedValueOnce(RESULT);
    render(<ModelCouncilTab />);

    fireEvent.change(screen.getByLabelText('Judge agent'), { target: { value: 'profile' } });
    fireEvent.change(screen.getByLabelText('Judge profile'), { target: { value: 'critic' } });
    fillQuestion();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Convene council' }));
    });

    expect(mockRunCouncil).toHaveBeenCalledWith({
      question: expect.any(String),
      member_models: ['default', 'default', 'default'],
      chair_model: 'critic-model',
    });
  });

  it('renders member answers side-by-side + the synthesis', async () => {
    mockRunCouncil.mockResolvedValueOnce(RESULT);
    render(<ModelCouncilTab />);
    fillQuestion();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Convene council' }));
    });

    await waitFor(() => {
      expect(screen.getByText('Council results')).toBeInTheDocument();
    });
    expect(screen.getByText('Paris is the capital.')).toBeInTheDocument();
    expect(screen.getByText('rate limited')).toBeInTheDocument();
    expect(screen.getByText('Answered')).toBeInTheDocument();
    expect(screen.getByText('Failed')).toBeInTheDocument();
    expect(
      screen.getByText('Both that answered agree: Paris. One seat failed.')
    ).toBeInTheDocument();
    expect(screen.getByText('by claude-opus-4-8')).toBeInTheDocument();
  });

  it('surfaces an error alert when the council run fails', async () => {
    mockRunCouncil.mockRejectedValueOnce(new Error('all member models failed to respond'));
    render(<ModelCouncilTab />);
    fillQuestion();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Convene council' }));
    });

    await waitFor(() => {
      const alert = screen.getByRole('alert');
      expect(alert.textContent).toMatch(/all member models failed to respond/);
    });
    expect(screen.queryByText('Council results')).not.toBeInTheDocument();
  });
});
