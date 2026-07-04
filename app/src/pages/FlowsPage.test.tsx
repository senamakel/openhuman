/**
 * FlowsPage (issue B5a / B5a.1 / B5b.1) — the Workflows list page. Asserts
 * the loading/empty/error/list states, that toggling a flow calls
 * `setFlowEnabled` and refreshes the row, that Run fires `runFlow`, shows a
 * "Workflow started" toast, and refetches the list, that "View runs" opens
 * `FlowRunsDrawer` for the clicked flow, that clicking a flow's name
 * navigates to its read-only Workflow Canvas (`/flows/:id`, issue B5b.1),
 * and that "New workflow" (header + empty state) navigates to Chat (no
 * canvas *builder* yet — bridges to B4's agent-proposal flow).
 */
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Flow } from '../services/api/flowsApi';
import { renderWithProviders } from '../test/test-utils';
import FlowsPage from './FlowsPage';

const listFlows = vi.hoisted(() => vi.fn());
const setFlowEnabled = vi.hoisted(() => vi.fn());
const runFlow = vi.hoisted(() => vi.fn());
const listFlowRuns = vi.hoisted(() => vi.fn());
vi.mock('../services/api/flowsApi', () => ({ listFlows, setFlowEnabled, runFlow, listFlowRuns }));

const mockNavigate = vi.hoisted(() => vi.fn());
vi.mock('react-router-dom', async importOriginal => {
  const actual = await importOriginal<typeof import('react-router-dom')>();
  return { ...actual, useNavigate: () => mockNavigate };
});

function makeFlow(overrides: Partial<Flow> = {}): Flow {
  return {
    id: 'flow-1',
    name: 'Daily digest',
    enabled: true,
    graph: { nodes: [], edges: [] },
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    last_run_at: null,
    last_status: null,
    require_approval: false,
    ...overrides,
  };
}

describe('FlowsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows a loading state while flows are being fetched', () => {
    listFlows.mockReturnValue(new Promise(() => {})); // never resolves
    renderWithProviders(<FlowsPage />);

    expect(screen.getByText('Loading workflows…')).toBeInTheDocument();
  });

  it('shows the empty state when there are no saved flows, with a "New workflow" action', async () => {
    listFlows.mockResolvedValue([]);
    renderWithProviders(<FlowsPage />);

    await waitFor(() => expect(screen.getByText('No workflows yet')).toBeInTheDocument());
    // There's no canvas builder yet (B5b) — the empty state's action bridges
    // to Chat/B4 instead, same as the header button.
    expect(screen.getByTestId('flows-empty-new-workflow')).toHaveTextContent('New workflow');
  });

  it('shows an error banner when the fetch fails', async () => {
    listFlows.mockRejectedValue(new Error('core unreachable'));
    renderWithProviders(<FlowsPage />);

    await waitFor(() =>
      expect(screen.getByText('Could not load workflows. Please try again.')).toBeInTheDocument()
    );
  });

  it('renders one row per saved flow', async () => {
    listFlows.mockResolvedValue([makeFlow(), makeFlow({ id: 'flow-2', name: 'Weekly report' })]);
    renderWithProviders(<FlowsPage />);

    await waitFor(() => expect(screen.getByText('Daily digest')).toBeInTheDocument());
    expect(screen.getByText('Weekly report')).toBeInTheDocument();
  });

  it('toggles a flow via setFlowEnabled and reflects the updated state', async () => {
    listFlows.mockResolvedValue([makeFlow({ enabled: true })]);
    setFlowEnabled.mockResolvedValue(makeFlow({ enabled: false }));
    renderWithProviders(<FlowsPage />);

    await waitFor(() => expect(screen.getByTestId('flow-toggle-flow-1')).toBeInTheDocument());
    fireEvent.click(screen.getByTestId('flow-toggle-flow-1'));

    expect(setFlowEnabled).toHaveBeenCalledWith('flow-1', false);
    await waitFor(() =>
      expect(screen.getByTestId('flow-status-flow-1')).toHaveTextContent('Paused')
    );
  });

  it('runs a flow, shows a "Workflow started" toast, and refetches the list', async () => {
    listFlows.mockResolvedValue([makeFlow()]);
    runFlow.mockResolvedValue({ output: null, pending_approvals: [], thread_id: 't1' });
    renderWithProviders(<FlowsPage />);

    await waitFor(() => expect(screen.getByTestId('flow-run-flow-1')).toBeInTheDocument());
    fireEvent.click(screen.getByTestId('flow-run-flow-1'));

    expect(runFlow).toHaveBeenCalledWith('flow-1');
    await waitFor(() => expect(screen.getByText('Workflow started')).toBeInTheDocument());
    // Loaded once on mount, once more on refetch after the run kicks off.
    await waitFor(() => expect(listFlows).toHaveBeenCalledTimes(2));
  });

  it('shows an error banner (without a toast) when runFlow rejects', async () => {
    listFlows.mockResolvedValue([makeFlow()]);
    runFlow.mockRejectedValue(new Error('flow disabled'));
    renderWithProviders(<FlowsPage />);

    await waitFor(() => expect(screen.getByTestId('flow-run-flow-1')).toBeInTheDocument());
    fireEvent.click(screen.getByTestId('flow-run-flow-1'));

    await waitFor(() => expect(screen.getByText('flow disabled')).toBeInTheDocument());
    expect(screen.queryByText('Workflow started')).not.toBeInTheDocument();
  });

  it('opens the run-history drawer for the clicked flow when "View runs" is clicked', async () => {
    listFlows.mockResolvedValue([makeFlow()]);
    listFlowRuns.mockResolvedValue([]);
    renderWithProviders(<FlowsPage />);

    await waitFor(() => expect(screen.getByTestId('flow-view-runs-flow-1')).toBeInTheDocument());
    fireEvent.click(screen.getByTestId('flow-view-runs-flow-1'));

    expect(await screen.findByTestId('flow-runs-drawer')).toBeInTheDocument();
    expect(screen.getByText('Runs for Daily digest')).toBeInTheDocument();
    expect(listFlowRuns).toHaveBeenCalledWith('flow-1');

    fireEvent.click(screen.getByTestId('flow-runs-close'));
    expect(screen.queryByTestId('flow-runs-drawer')).not.toBeInTheDocument();
  });

  it('navigates to the Workflow Canvas when a flow name is clicked', async () => {
    listFlows.mockResolvedValue([makeFlow()]);
    renderWithProviders(<FlowsPage />);

    await waitFor(() => expect(screen.getByTestId('flow-view-flow-1')).toBeInTheDocument());
    fireEvent.click(screen.getByTestId('flow-view-flow-1'));

    expect(mockNavigate).toHaveBeenCalledWith('/flows/flow-1');
  });

  it('renders a "New workflow" header button and navigates to /chat when clicked', async () => {
    listFlows.mockResolvedValue([makeFlow()]);
    renderWithProviders(<FlowsPage />);

    const newWorkflowButton = await screen.findByTestId('flows-new-workflow');
    expect(newWorkflowButton).toHaveTextContent('New workflow');
    fireEvent.click(newWorkflowButton);

    expect(mockNavigate).toHaveBeenCalledWith('/chat');
  });

  it('navigates to /chat when the empty-state "New workflow" action is clicked', async () => {
    listFlows.mockResolvedValue([]);
    renderWithProviders(<FlowsPage />);

    const emptyStateButton = await screen.findByTestId('flows-empty-new-workflow');
    fireEvent.click(emptyStateButton);

    expect(mockNavigate).toHaveBeenCalledWith('/chat');
  });
});
