import { configureStore } from '@reduxjs/toolkit';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { Provider } from 'react-redux';
import { describe, expect, it, vi } from 'vitest';

import chatRuntimeReducer, { recordChatTurnUsage } from '../../store/chatRuntimeSlice';
import ComposerTokenStats from './ComposerTokenStats';

vi.mock('../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (k: string) => k }) }));

function renderWithUsage(
  payloads: Array<Parameters<typeof recordChatTurnUsage>[0]>,
  props?: { model?: string | null }
) {
  const store = configureStore({ reducer: { chatRuntime: chatRuntimeReducer } });
  for (const p of payloads) store.dispatch(recordChatTurnUsage(p));
  return render(
    <Provider store={store}>
      <ComposerTokenStats {...props} />
    </Provider>
  );
}

describe('<ComposerTokenStats />', () => {
  it('renders nothing before any turn when no model is provided', () => {
    const { container } = renderWithUsage([]);
    expect(container).toBeEmptyDOMElement();
  });

  it('shows token counts, context window, and USD cost after a turn', () => {
    renderWithUsage([
      { inputTokens: 1200, outputTokens: 300, costUsd: 0.0123, contextWindow: 200_000 },
    ]);
    // IN / OUT labels with formatted counts.
    expect(screen.getByText(/token\.inLabel/)).toHaveTextContent('1.2K');
    expect(screen.getByText(/token\.outLabel/)).toHaveTextContent('300');
    // Context window uses the real reported window (200K), not just a default.
    expect(screen.getByText(/token\.ctxLabel/)).toHaveTextContent('200K');
    // USD cost rendered.
    expect(screen.getByText('$0.012')).toBeInTheDocument();
  });

  it('reveals the per-sub-agent breakdown on hover', () => {
    renderWithUsage([
      {
        inputTokens: 500,
        outputTokens: 100,
        costUsd: 0.01,
        subAgents: [{ agentId: 'researcher', inputTokens: 200, outputTokens: 40, costUsd: 0.004 }],
      },
    ]);
    // Breakdown is hidden until hover.
    expect(screen.queryByTestId('composer-token-breakdown')).not.toBeInTheDocument();

    fireEvent.mouseEnter(screen.getByRole('group'));
    const breakdown = screen.getByTestId('composer-token-breakdown');
    expect(within(breakdown).getByText('researcher')).toBeInTheDocument();
    // 200 + 40 = 240 combined tokens, cost, and a single run.
    expect(within(breakdown).getByText(/240/)).toBeInTheDocument();
    expect(within(breakdown).getByText(/\$0\.004/)).toBeInTheDocument();
    expect(within(breakdown).getByText(/1×/)).toBeInTheDocument();
  });

  it('shows a no-sub-agents note when none ran', () => {
    renderWithUsage([{ inputTokens: 100, outputTokens: 20, costUsd: 0.001 }]);
    fireEvent.mouseEnter(screen.getByRole('group'));
    const breakdown = screen.getByTestId('composer-token-breakdown');
    expect(within(breakdown).getByText('token.noSubAgents')).toBeInTheDocument();
  });

  it('still renders the model when no turns have run yet', () => {
    renderWithUsage([], { model: 'reasoning-v1' });
    expect(screen.getByText('reasoning-v1')).toBeInTheDocument();
  });
});
