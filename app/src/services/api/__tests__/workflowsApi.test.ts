import { beforeEach, describe, expect, it, vi } from 'vitest';

import { workflowsApi } from '../workflowsApi';

vi.mock('../../coreRpcClient', () => ({ callCoreRpc: vi.fn() }));

async function mockRpc() {
  const { callCoreRpc } = await import('../../coreRpcClient');
  vi.mocked(callCoreRpc).mockReset();
  return vi.mocked(callCoreRpc);
}

const DETAIL = {
  id: 'bug-triage',
  name: 'bug-triage',
  description: 'Handle a bug',
  when_to_use: 'a user reports a bug',
  tags: ['support'],
  tools: null,
  phases: {
    on_pick_up_task: {
      description: null,
      rules: ['Reproduce first'],
      scripts: [],
      tools: null,
      context: [],
    },
  },
  scope: 'user' as const,
  location: '/home/u/.openhuman/workflows/bug-triage/WORKFLOW.md',
  warnings: [],
};

describe('workflowsApi', () => {
  beforeEach(async () => {
    await mockRpc();
  });

  it('listWorkflows unwraps the envelope and defaults to an empty array', async () => {
    const rpc = await mockRpc();
    rpc.mockResolvedValueOnce({ data: { workflows: [] } });
    const out = await workflowsApi.listWorkflows();
    expect(rpc).toHaveBeenCalledWith({ method: 'openhuman.workflows_list' });
    expect(out).toEqual([]);
  });

  it('createWorkflow forwards params with a default scope', async () => {
    const rpc = await mockRpc();
    rpc.mockResolvedValueOnce({ workflow: DETAIL });
    const out = await workflowsApi.createWorkflow({
      name: 'Bug triage',
      description: 'Handle a bug',
      when_to_use: 'a user reports a bug',
    });
    expect(rpc).toHaveBeenCalledWith({
      method: 'openhuman.workflows_create',
      params: {
        name: 'Bug triage',
        description: 'Handle a bug',
        when_to_use: 'a user reports a bug',
        scope: 'user',
        tags: [],
      },
    });
    expect(out.id).toBe('bug-triage');
    expect(out.phases.on_pick_up_task.rules).toEqual(['Reproduce first']);
  });

  it('readWorkflow returns the workflow detail', async () => {
    const rpc = await mockRpc();
    rpc.mockResolvedValueOnce({ workflow: DETAIL });
    const out = await workflowsApi.readWorkflow('bug-triage');
    expect(rpc).toHaveBeenCalledWith({
      method: 'openhuman.workflows_read',
      params: { workflow_id: 'bug-triage' },
    });
    expect(out.when_to_use).toBe('a user reports a bug');
  });

  it('resolvePhase passes workflow_id and phase', async () => {
    const rpc = await mockRpc();
    rpc.mockResolvedValueOnce({
      workflow_id: 'bug-triage',
      phase: 'on_close_task',
      declared: true,
      guidance: '## guidance',
      tool_scope: null,
      context: [],
      scripts: [],
    });
    const out = await workflowsApi.resolvePhase('bug-triage', 'on_close_task');
    expect(rpc).toHaveBeenCalledWith({
      method: 'openhuman.workflows_phase',
      params: { workflow_id: 'bug-triage', phase: 'on_close_task' },
    });
    expect(out.declared).toBe(true);
  });

  it('uninstallWorkflow forwards the name', async () => {
    const rpc = await mockRpc();
    rpc.mockResolvedValueOnce({ name: 'bug-triage', removed_path: '/x', scope: 'user' });
    const out = await workflowsApi.uninstallWorkflow('bug-triage');
    expect(rpc).toHaveBeenCalledWith({
      method: 'openhuman.workflows_uninstall',
      params: { name: 'bug-triage' },
    });
    expect(out.removed_path).toBe('/x');
  });
});
