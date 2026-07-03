/**
 * FlowRunInspectorDrawer (issue B3b) — rendering contract.
 *
 * Asserts: renders null when `runId` is null; loading state; renders fetched
 * run data (status pill, steps, expandable output, port pill); error state;
 * pending-approvals banner when `status === 'pending_approval'`; run.error
 * banner; Escape and backdrop both close; close button calls `onClose`.
 *
 * Mocks `useFlowRunPoller` directly rather than the underlying RPC client —
 * its own poll-until-terminal contract is covered by
 * `hooks/__tests__/useFlowRunPoller.test.ts`.
 */
import { fireEvent, render, screen } from '@testing-library/react';
import { Provider } from 'react-redux';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { FlowRun } from '../../../services/api/flowsApi';
import { store } from '../../../store';
import { FlowRunInspectorDrawer } from '../FlowRunInspectorDrawer';

const useFlowRunPoller = vi.hoisted(() => vi.fn());
vi.mock('../../../hooks/useFlowRunPoller', () => ({ useFlowRunPoller }));

function makeRun(overrides: Partial<FlowRun> = {}): FlowRun {
  return {
    id: 'thread-1',
    flow_id: 'flow-1',
    thread_id: 'thread-1',
    status: 'running',
    started_at: '2026-01-01T00:00:00Z',
    steps: [
      { node_id: 'fetch-data', output: { rows: 3 } },
      { node_id: 'branch', output: 'ok', port: 'true' },
    ],
    pending_approvals: [],
    ...overrides,
  };
}

function renderDrawer(runId: string | null, onClose: () => void) {
  return render(
    <Provider store={store}>
      <FlowRunInspectorDrawer runId={runId} onClose={onClose} />
    </Provider>
  );
}

describe('FlowRunInspectorDrawer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders null when runId is null', () => {
    useFlowRunPoller.mockReturnValue({ run: null, loading: false, error: null });
    const { container } = renderDrawer(null, vi.fn());
    expect(container).toBeEmptyDOMElement();
    expect(useFlowRunPoller).toHaveBeenCalledWith(null);
  });

  it('shows a loading state before data resolves', () => {
    useFlowRunPoller.mockReturnValue({ run: null, loading: true, error: null });
    renderDrawer('thread-1', vi.fn());
    expect(screen.getByTestId('flow-run-inspector-loading')).toBeInTheDocument();
  });

  it('renders the run status pill and step list once data resolves', () => {
    useFlowRunPoller.mockReturnValue({ run: makeRun(), loading: false, error: null });
    renderDrawer('thread-1', vi.fn());

    expect(screen.getByTestId('flow-run-status-pill')).toHaveTextContent('Running');
    expect(screen.getByTestId('flow-run-steps')).toBeInTheDocument();
    expect(screen.getByText('fetch-data')).toBeInTheDocument();
    expect(screen.getByText('branch')).toBeInTheDocument();
    expect(screen.getByTestId('flow-run-step-port-1')).toHaveTextContent('true');
  });

  it('expands a step to reveal its output', () => {
    useFlowRunPoller.mockReturnValue({ run: makeRun(), loading: false, error: null });
    renderDrawer('thread-1', vi.fn());

    const step = screen.getByTestId('flow-run-step-0');
    expect(step.querySelector('pre')).not.toBeVisible();
    fireEvent.click(screen.getAllByText('Output')[0]);
    expect(step.querySelector('pre')).toBeVisible();
    expect(step.querySelector('pre')?.textContent).toContain('"rows": 3');
  });

  it('shows an error state when the poller reports an error', () => {
    useFlowRunPoller.mockReturnValue({ run: null, loading: false, error: 'network down' });
    renderDrawer('thread-1', vi.fn());
    expect(screen.getByTestId('flow-run-inspector-error')).toHaveTextContent('network down');
  });

  it('shows the pending-approvals banner when status is pending_approval', () => {
    useFlowRunPoller.mockReturnValue({
      run: makeRun({ status: 'pending_approval', pending_approvals: ['node-a', 'node-b'] }),
      loading: false,
      error: null,
    });
    renderDrawer('thread-1', vi.fn());
    expect(screen.getByTestId('flow-run-pending-approvals-banner')).toHaveTextContent('2');
  });

  it('does not show the pending-approvals banner for a running run', () => {
    useFlowRunPoller.mockReturnValue({ run: makeRun(), loading: false, error: null });
    renderDrawer('thread-1', vi.fn());
    expect(screen.queryByTestId('flow-run-pending-approvals-banner')).not.toBeInTheDocument();
  });

  it('shows the run.error banner when present', () => {
    useFlowRunPoller.mockReturnValue({
      run: makeRun({ status: 'failed', error: 'node crashed' }),
      loading: false,
      error: null,
    });
    renderDrawer('thread-1', vi.fn());
    expect(screen.getByTestId('flow-run-error-banner')).toHaveTextContent('node crashed');
  });

  it('calls onClose when the close button is clicked', () => {
    useFlowRunPoller.mockReturnValue({ run: makeRun(), loading: false, error: null });
    const onClose = vi.fn();
    renderDrawer('thread-1', onClose);
    fireEvent.click(screen.getByTestId('flow-run-inspector-close'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('calls onClose when the backdrop is clicked', () => {
    useFlowRunPoller.mockReturnValue({ run: makeRun(), loading: false, error: null });
    const onClose = vi.fn();
    renderDrawer('thread-1', onClose);
    fireEvent.click(screen.getByTestId('flow-run-inspector-backdrop'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('calls onClose when Escape is pressed', () => {
    useFlowRunPoller.mockReturnValue({ run: makeRun(), loading: false, error: null });
    const onClose = vi.fn();
    renderDrawer('thread-1', onClose);
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
