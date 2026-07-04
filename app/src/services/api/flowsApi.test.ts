import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  getFlowRun,
  listFlowRuns,
  listFlows,
  resumeFlow,
  runFlow,
  setFlowEnabled,
} from './flowsApi';

const mockCallCoreRpc = vi.fn();
vi.mock('../coreRpcClient', () => ({ callCoreRpc: (...a: unknown[]) => mockCallCoreRpc(...a) }));

/** Every `flows_*` handler wraps its payload via `RpcOutcome::single_log`. */
function cliEnvelope<T>(
  result: T,
  logs: string[] = ['did something']
): { result: T; logs: string[] } {
  return { result, logs };
}

describe('flowsApi', () => {
  beforeEach(() => {
    mockCallCoreRpc.mockReset();
  });

  describe('resumeFlow', () => {
    it('calls openhuman.flows_resume with id, thread_id, approvals', async () => {
      mockCallCoreRpc.mockResolvedValue(
        cliEnvelope({ output: { nodes: {} }, pending_approvals: [], thread_id: 't1' })
      );

      const result = await resumeFlow('flow-1', 't1', ['node-a']);

      expect(mockCallCoreRpc).toHaveBeenCalledWith({
        method: 'openhuman.flows_resume',
        params: { id: 'flow-1', thread_id: 't1', approvals: ['node-a'] },
        // flows_resume can run ~600s server-side, so the client budget is raised.
        timeoutMs: 610_000,
      });
      expect(result).toEqual({ output: { nodes: {} }, pending_approvals: [], thread_id: 't1' });
    });

    it('unwraps the { result, logs } envelope', async () => {
      const payload = { output: null, pending_approvals: ['node-b'], thread_id: 't2' };
      mockCallCoreRpc.mockResolvedValue(cliEnvelope(payload));

      const result = await resumeFlow('flow-1', 't2', ['node-b']);

      expect(result).toEqual(payload);
    });

    it('passes through a bare (unwrapped) payload unchanged', async () => {
      const payload = { output: null, pending_approvals: [], thread_id: 't3' };
      mockCallCoreRpc.mockResolvedValue(payload);

      const result = await resumeFlow('flow-1', 't3', []);

      expect(result).toEqual(payload);
    });

    it('propagates rejection from callCoreRpc', async () => {
      mockCallCoreRpc.mockRejectedValue(new Error('no pending approval matches'));

      await expect(resumeFlow('flow-1', 't1', ['wrong-node'])).rejects.toThrow(
        'no pending approval matches'
      );
    });
  });

  describe('listFlowRuns', () => {
    it('calls openhuman.flows_list_runs with id', async () => {
      mockCallCoreRpc.mockResolvedValue(cliEnvelope([]));

      await listFlowRuns('flow-1');

      expect(mockCallCoreRpc).toHaveBeenCalledWith({
        method: 'openhuman.flows_list_runs',
        params: { id: 'flow-1' },
      });
    });

    it('passes limit when provided', async () => {
      mockCallCoreRpc.mockResolvedValue(cliEnvelope([]));

      await listFlowRuns('flow-1', 5);

      expect(mockCallCoreRpc).toHaveBeenCalledWith({
        method: 'openhuman.flows_list_runs',
        params: { id: 'flow-1', limit: 5 },
      });
    });

    it('unwraps the { result, logs } envelope into the run array', async () => {
      const runs = [
        {
          id: 't1',
          flow_id: 'flow-1',
          thread_id: 't1',
          status: 'completed' as const,
          started_at: '2026-01-01T00:00:00Z',
          finished_at: '2026-01-01T00:01:00Z',
          steps: [],
          pending_approvals: [],
          error: null,
        },
      ];
      mockCallCoreRpc.mockResolvedValue(cliEnvelope(runs));

      const result = await listFlowRuns('flow-1');

      expect(result).toEqual(runs);
    });

    it('propagates rejection from callCoreRpc', async () => {
      mockCallCoreRpc.mockRejectedValue(new Error('boom'));

      await expect(listFlowRuns('flow-1')).rejects.toThrow('boom');
    });
  });

  describe('getFlowRun', () => {
    it('calls openhuman.flows_get_run with run_id', async () => {
      const run = {
        id: 't1',
        flow_id: 'flow-1',
        thread_id: 't1',
        status: 'pending_approval' as const,
        started_at: '2026-01-01T00:00:00Z',
        finished_at: null,
        steps: [{ node_id: 'n1', output: { ok: true } }],
        pending_approvals: ['n2'],
        error: null,
      };
      mockCallCoreRpc.mockResolvedValue(cliEnvelope(run));

      const result = await getFlowRun('t1');

      expect(mockCallCoreRpc).toHaveBeenCalledWith({
        method: 'openhuman.flows_get_run',
        params: { run_id: 't1' },
      });
      expect(result).toEqual(run);
    });

    it('propagates rejection from callCoreRpc', async () => {
      mockCallCoreRpc.mockRejectedValue(new Error('flow run not found'));

      await expect(getFlowRun('missing')).rejects.toThrow('flow run not found');
    });
  });

  describe('listFlows', () => {
    const flow = {
      id: 'flow-1',
      name: 'Demo flow',
      enabled: true,
      graph: { nodes: [], edges: [] },
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
      last_run_at: null,
      last_status: null,
      require_approval: false,
    };

    it('calls openhuman.flows_list with no params', async () => {
      mockCallCoreRpc.mockResolvedValue(cliEnvelope([flow]));

      await listFlows();

      expect(mockCallCoreRpc).toHaveBeenCalledWith({ method: 'openhuman.flows_list', params: {} });
    });

    it('unwraps the { result, logs } envelope into the flow array', async () => {
      mockCallCoreRpc.mockResolvedValue(cliEnvelope([flow]));

      const result = await listFlows();

      expect(result).toEqual([flow]);
    });

    it('propagates rejection from callCoreRpc', async () => {
      mockCallCoreRpc.mockRejectedValue(new Error('boom'));

      await expect(listFlows()).rejects.toThrow('boom');
    });
  });

  describe('setFlowEnabled', () => {
    it('calls openhuman.flows_set_enabled with id and enabled', async () => {
      const flow = {
        id: 'flow-1',
        name: 'Demo flow',
        enabled: false,
        graph: {},
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
        last_run_at: null,
        last_status: null,
        require_approval: false,
      };
      mockCallCoreRpc.mockResolvedValue(cliEnvelope(flow));

      const result = await setFlowEnabled('flow-1', false);

      expect(mockCallCoreRpc).toHaveBeenCalledWith({
        method: 'openhuman.flows_set_enabled',
        params: { id: 'flow-1', enabled: false },
      });
      expect(result).toEqual(flow);
    });

    it('propagates rejection from callCoreRpc', async () => {
      mockCallCoreRpc.mockRejectedValue(new Error('flow not found'));

      await expect(setFlowEnabled('missing', true)).rejects.toThrow('flow not found');
    });
  });

  describe('runFlow', () => {
    it('calls openhuman.flows_run with id, input, and the extended timeout', async () => {
      mockCallCoreRpc.mockResolvedValue(
        cliEnvelope({ output: { nodes: {} }, pending_approvals: [], thread_id: 't1' })
      );

      const result = await runFlow('flow-1');

      expect(mockCallCoreRpc).toHaveBeenCalledWith({
        method: 'openhuman.flows_run',
        params: { id: 'flow-1', input: null },
        timeoutMs: 610_000,
      });
      expect(result).toEqual({ output: { nodes: {} }, pending_approvals: [], thread_id: 't1' });
    });

    it('passes a supplied input payload through', async () => {
      mockCallCoreRpc.mockResolvedValue(
        cliEnvelope({ output: null, pending_approvals: [], thread_id: 't2' })
      );

      await runFlow('flow-1', { trigger: 'manual' });

      expect(mockCallCoreRpc).toHaveBeenCalledWith({
        method: 'openhuman.flows_run',
        params: { id: 'flow-1', input: { trigger: 'manual' } },
        timeoutMs: 610_000,
      });
    });

    it('unwraps the { result, logs } envelope', async () => {
      const payload = { output: null, pending_approvals: ['node-a'], thread_id: 't3' };
      mockCallCoreRpc.mockResolvedValue(cliEnvelope(payload));

      const result = await runFlow('flow-1');

      expect(result).toEqual(payload);
    });

    it('propagates rejection from callCoreRpc', async () => {
      mockCallCoreRpc.mockRejectedValue(new Error('flow disabled'));

      await expect(runFlow('flow-1')).rejects.toThrow('flow disabled');
    });
  });
});
