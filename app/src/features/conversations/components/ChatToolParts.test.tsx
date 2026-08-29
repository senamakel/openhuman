import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import type { SubagentActivity } from '../../../store/chatRuntimeSlice';
import { ChatToolFallback, ChatToolGroup } from './ChatToolParts';

const activity: SubagentActivity = {
  taskId: 'sub-1',
  agentId: 'researcher',
  displayName: 'Researcher',
  toolCalls: [],
  transcript: [{ kind: 'thinking', text: 'Checking primary sources.' }],
};

describe('ChatToolParts', () => {
  it('renders a delegation with progress args and no result as running', () => {
    render(
      <ChatToolFallback
        type="tool-call"
        toolName="task"
        toolCallId="sub-1"
        args={{ progress: activity } as never}
        argsText="{}"
        result={undefined}
        status={{ type: 'running' }}
        addResult={() => {}}
        resume={() => {}}
        respondToApproval={() => {}}
      />
    );

    expect(screen.getByText('running')).toBeInTheDocument();
    expect(screen.getByText('Researcher')).toBeInTheDocument();
    expect(screen.getByText('Checking primary sources.')).toBeInTheDocument();
  });

  it('opens a group containing in-flight work on mount', () => {
    render(
      <ChatToolGroup group={{ type: 'group-tool-call', status: { type: 'running' }, indices: [0] }}>
        <span>live delegation</span>
      </ChatToolGroup>
    );

    expect(screen.getByText('live delegation')).toBeVisible();
  });
});
