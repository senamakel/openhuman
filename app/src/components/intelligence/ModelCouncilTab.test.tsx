import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { CouncilMemberResult, ModelCouncilResult } from '../../services/api/modelCouncilApi';
import ModelCouncilTab from './ModelCouncilTab';

const mockAnswerMember = vi.fn();
const mockSynthesizeCouncil = vi.fn();
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
  modelCouncilApi: {
    answerMember: (...args: unknown[]) => mockAnswerMember(...args),
    synthesizeCouncil: (...args: unknown[]) => mockSynthesizeCouncil(...args),
  },
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

const DEFAULT_MEMBERS: CouncilMemberResult[] = [
  { model: 'default', response: 'Paris is the capital.', error: null },
  { model: 'default', response: 'France uses Paris as its capital.', error: null },
  { model: 'default', response: 'The answer is Paris.', error: null },
];

const fillQuestion = () => {
  fireEvent.change(screen.getByLabelText('Question'), {
    target: { value: 'What is the capital of France?' },
  });
};

const mockProgressiveSuccess = (members: CouncilMemberResult[] = DEFAULT_MEMBERS) => {
  mockAnswerMember.mockImplementation(async ({ model }: { model: string }) => {
    const index = mockAnswerMember.mock.calls.length - 1;
    return members[index] ?? { model, response: `answer ${index + 1}`, error: null };
  });
  mockSynthesizeCouncil.mockResolvedValue(RESULT);
};

describe('ModelCouncilTab', () => {
  beforeEach(() => {
    mockAnswerMember.mockReset();
    mockSynthesizeCouncil.mockReset();
    mockDispatch.mockReset();
  });

  it('renders settings, shared reasoning, and three Rive council seats by default', () => {
    render(<ModelCouncilTab />);

    expect(screen.getByText('Model Council')).toBeInTheDocument();
    expect(screen.getByText('Council settings')).toBeInTheDocument();
    expect(screen.getByLabelText('Debate turns')).toHaveValue('3');
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
    let resolveFirst: (value: CouncilMemberResult) => void = () => {};
    let resolveSecond: (value: CouncilMemberResult) => void = () => {};
    let resolveThird: (value: CouncilMemberResult) => void = () => {};
    let resolveSynthesis: (value: ModelCouncilResult) => void = () => {};
    mockAnswerMember
      .mockImplementation(async ({ model }: { model: string }) => ({
        model,
        response: `follow-up thought ${mockAnswerMember.mock.calls.length}`,
        error: null,
      }))
      .mockReturnValueOnce(
        new Promise<CouncilMemberResult>(resolve => {
          resolveFirst = resolve;
        })
      )
      .mockReturnValueOnce(
        new Promise<CouncilMemberResult>(resolve => {
          resolveSecond = resolve;
        })
      )
      .mockReturnValueOnce(
        new Promise<CouncilMemberResult>(resolve => {
          resolveThird = resolve;
        })
      );
    mockSynthesizeCouncil.mockReturnValueOnce(
      new Promise<ModelCouncilResult>(resolve => {
        resolveSynthesis = resolve;
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
      resolveFirst({ model: 'default', response: 'First juror live thought: Paris.', error: null });
    });

    expect(screen.getByText('First juror live thought: Paris.')).toBeInTheDocument();
    expect(screen.getByText('Round 1')).toBeInTheDocument();
    expect(screen.getAllByText('Thinking')).toHaveLength(2);
    expect(screen.getByText('Answered')).toBeInTheDocument();
    expect(screen.getByText('Judge')).toBeInTheDocument();

    await act(async () => {
      resolveSecond({ model: 'default', response: 'Second juror agrees.', error: null });
      resolveThird({ model: 'default', response: 'Third juror agrees.', error: null });
    });

    await waitFor(() => {
      expect(screen.getByText('Synthesizing')).toBeInTheDocument();
    });

    await act(async () => {
      resolveSynthesis(RESULT);
    });

    await waitFor(() => {
      expect(screen.queryByText('Council deliberation')).not.toBeInTheDocument();
    });
  });

  it('streams failed juror status without blocking other juror thoughts', async () => {
    let resolveFirst: (value: CouncilMemberResult) => void = () => {};
    let resolveSecond: (value: CouncilMemberResult) => void = () => {};
    let resolveThird: (value: CouncilMemberResult) => void = () => {};
    mockAnswerMember
      .mockImplementation(async ({ model }: { model: string }) => ({
        model,
        response: `follow-up answer ${mockAnswerMember.mock.calls.length}`,
        error: null,
      }))
      .mockReturnValueOnce(
        new Promise<CouncilMemberResult>(resolve => {
          resolveFirst = resolve;
        })
      )
      .mockReturnValueOnce(
        new Promise<CouncilMemberResult>(resolve => {
          resolveSecond = resolve;
        })
      )
      .mockReturnValueOnce(
        new Promise<CouncilMemberResult>(resolve => {
          resolveThird = resolve;
        })
      );
    mockSynthesizeCouncil.mockResolvedValueOnce(RESULT);
    render(<ModelCouncilTab />);
    fillQuestion();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Convene council' }));
    });

    await act(async () => {
      resolveFirst({ model: 'default', response: null, error: 'rate limited' });
    });

    expect(screen.getByText('rate limited')).toBeInTheDocument();
    expect(screen.getByText('Failed')).toBeInTheDocument();
    expect(screen.getAllByText('Thinking')).toHaveLength(2);

    await act(async () => {
      resolveSecond({ model: 'default', response: 'Second juror answer.', error: null });
      resolveThird({ model: 'default', response: 'Third juror answer.', error: null });
    });

    await waitFor(() => {
      expect(mockSynthesizeCouncil).toHaveBeenCalledWith({
        question: expect.any(String),
        members: [
          {
            model: 'default',
            response: expect.stringContaining('[failed: rate limited]'),
            error: null,
          },
          {
            model: 'default',
            response: expect.stringContaining('Second juror answer.'),
            error: null,
          },
          {
            model: 'default',
            response: expect.stringContaining('Third juror answer.'),
            error: null,
          },
        ],
        chair_model: 'default',
      });
    });
  });

  it('appends juror turns to the shared scratchpad before the next debate round', async () => {
    mockAnswerMember.mockImplementation(async ({ model }: { model: string }) => ({
      model,
      response: `round ${mockAnswerMember.mock.calls.length} update`,
      error: null,
    }));
    mockSynthesizeCouncil.mockResolvedValueOnce(RESULT);
    render(<ModelCouncilTab />);
    fillQuestion();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Convene council' }));
    });

    await waitFor(() => {
      expect(mockSynthesizeCouncil).toHaveBeenCalled();
    });
    const scratchpadValue = (screen.getByLabelText('Shared reasoning file') as HTMLTextAreaElement)
      .value;
    expect(scratchpadValue).toContain('## Round 1 updates');
    expect(scratchpadValue).toContain('round 1 update');
    expect(mockAnswerMember.mock.calls[3][0].question).toContain('Round 1 updates');
    expect(mockAnswerMember.mock.calls[3][0].question).toContain('round 1 update');
  });

  it('lets a council seat use a saved profile and submits that profile model', async () => {
    mockProgressiveSuccess();
    render(<ModelCouncilTab />);

    const firstSeat = screen.getByLabelText('Juror 1 name').closest('article');
    expect(firstSeat).not.toBeNull();
    fireEvent.click(within(firstSeat as HTMLElement).getByRole('tab', { name: 'Profile' }));
    fireEvent.change(screen.getByLabelText('Juror 1 profile'), { target: { value: 'critic' } });
    fillQuestion();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Convene council' }));
    });

    expect(mockAnswerMember.mock.calls.map(call => call[0].model)).toEqual([
      'critic-model',
      'default',
      'default',
      'critic-model',
      'default',
      'default',
      'critic-model',
      'default',
      'default',
    ]);
    expect(mockSynthesizeCouncil).toHaveBeenCalledWith({
      question: expect.stringContaining('shared_reasoning.md'),
      members: expect.any(Array),
      chair_model: 'default',
    });
    expect(mockAnswerMember.mock.calls[0][0].question).toContain('User question:');
    expect(mockAnswerMember.mock.calls[0][0].question).toContain('What is the capital of France?');
    expect(mockAnswerMember.mock.calls[0][0].question).toContain('Debate round 1 of 3.');
    expect(mockAnswerMember.mock.calls[8][0].question).toContain('Debate round 3 of 3.');
  });

  it('lets the judge agent use a saved profile unless a model override is typed', async () => {
    mockProgressiveSuccess();
    render(<ModelCouncilTab />);

    fireEvent.change(screen.getByLabelText('Judge agent'), { target: { value: 'profile' } });
    fireEvent.change(screen.getByLabelText('Judge profile'), { target: { value: 'critic' } });
    fillQuestion();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Convene council' }));
    });

    expect(mockAnswerMember.mock.calls.map(call => call[0].model)).toEqual([
      'default',
      'default',
      'default',
      'default',
      'default',
      'default',
      'default',
      'default',
      'default',
    ]);
    expect(mockSynthesizeCouncil).toHaveBeenCalledWith({
      question: expect.any(String),
      members: expect.any(Array),
      chair_model: 'critic-model',
    });
  });

  it('renders member answers side-by-side + the synthesis', async () => {
    mockProgressiveSuccess(RESULT.members);
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
    expect(screen.getByText('Debate usage')).toBeInTheDocument();
    expect(screen.getByText('Total')).toBeInTheDocument();
  });

  it('renders council markdown instead of showing raw markdown markers', async () => {
    mockProgressiveSuccess([
      { model: 'default', response: '**Paris** is the capital.', error: null },
      { model: 'default', response: '- France\n- Paris', error: null },
      { model: 'default', response: '`Paris` remains the answer.', error: null },
    ]);
    mockSynthesizeCouncil.mockResolvedValueOnce({
      ...RESULT,
      members: [
        { model: 'default', response: '**Paris** is the capital.', error: null },
        { model: 'default', response: '- France\n- Paris', error: null },
      ],
      synthesis: '## Consensus\n\nThe answer is **Paris**.',
    });
    render(<ModelCouncilTab />);
    fillQuestion();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Convene council' }));
    });

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Consensus' })).toBeInTheDocument();
    });
    const results = screen.getByText('Council results').closest('section');
    expect(results).not.toBeNull();
    expect(screen.getAllByText('Paris').some(node => node.tagName.toLowerCase() === 'strong')).toBe(
      true
    );
    expect(within(results as HTMLElement).queryByText(/\*\*Paris\*\*/)).not.toBeInTheDocument();
  });

  it('surfaces an error alert when the council run fails', async () => {
    mockAnswerMember.mockResolvedValue({ model: 'default', response: null, error: 'downstream' });
    mockSynthesizeCouncil.mockRejectedValueOnce(new Error('all member models failed to respond'));
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
