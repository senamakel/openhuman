import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { SubagentActivity } from '../../../../store/chatRuntimeSlice';
import { SubagentDrawer } from '../SubagentDrawer';

function activity(overrides: Partial<SubagentActivity> = {}): SubagentActivity {
  return {
    taskId: 'sub-1',
    agentId: 'researcher',
    toolCalls: [],
    streamingText: '',
    streamingThinking: '',
    ...overrides,
  };
}

describe('SubagentDrawer', () => {
  it('renders nothing when no subagent is selected', () => {
    const { container } = render(<SubagentDrawer subagent={null} onClose={() => {}} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders the streamed reasoning and visible output', () => {
    render(
      <SubagentDrawer
        subagent={activity({
          streamingThinking: 'comparing the two sources',
          streamingText: 'The answer is **42**.',
        })}
        status="running"
        onClose={() => {}}
      />
    );
    const drawer = screen.getByTestId('subagent-drawer');
    expect(drawer.textContent).toContain('researcher');
    expect(drawer.textContent).toContain('comparing the two sources');
    expect(drawer.textContent).toContain('The answer is');
  });

  it('shows a working placeholder while running with no output yet', () => {
    render(<SubagentDrawer subagent={activity()} status="running" onClose={() => {}} />);
    expect(screen.getByTestId('subagent-drawer').textContent).toContain('Working');
  });

  it('lists child tool calls with status', () => {
    render(
      <SubagentDrawer
        subagent={activity({
          toolCalls: [
            { callId: 'c1', toolName: 'web_search', status: 'success', elapsedMs: 1200 },
            { callId: 'c2', toolName: 'composio_execute', status: 'running', iteration: 2 },
          ],
        })}
        status="running"
        onClose={() => {}}
      />
    );
    const calls = screen.getAllByTestId('subagent-drawer-tool-call');
    expect(calls).toHaveLength(2);
    expect(calls[0].textContent).toContain('web_search');
    expect(calls[0].textContent).toContain('1.2s');
    expect(calls[1].textContent).toContain('running');
  });

  it('invokes onClose from the close button', async () => {
    const onClose = vi.fn();
    render(<SubagentDrawer subagent={activity()} status="success" onClose={onClose} />);
    // Two affordances carry the close label (backdrop + ✕ button); click the
    // explicit ✕ control.
    await userEvent.click(screen.getByText('✕'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
