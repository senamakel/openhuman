/**
 * FlowCanvasPage (issue B5b.1) — the read-only Workflow Canvas view at
 * `/flows/:id`. Asserts the loading → canvas happy path, the not-found state
 * (mirrors the Rust `flows_get` "not found" error), and the generic error
 * state for any other failure.
 */
import { render, screen, waitFor } from '@testing-library/react';
import { createMemoryRouter, MemoryRouter, Route, RouterProvider, Routes } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Flow } from '../../services/api/flowsApi';
import FlowCanvasPage from '../FlowCanvasPage';

const getFlow = vi.hoisted(() => vi.fn());
vi.mock('../../services/api/flowsApi', () => ({ getFlow }));

function makeFlow(overrides: Partial<Flow> = {}): Flow {
  return {
    id: 'test-id',
    name: 'Daily digest',
    enabled: true,
    graph: {
      schema_version: 1,
      id: 'test-id',
      name: 'Daily digest',
      nodes: [
        {
          id: 't',
          kind: 'trigger',
          name: 'Start',
          config: {},
          ports: [],
          position: { x: 0, y: 0 },
        },
      ],
      edges: [],
    },
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    last_run_at: null,
    last_status: null,
    require_approval: false,
    ...overrides,
  };
}

function renderAtFlowId(id: string) {
  return render(
    <MemoryRouter initialEntries={[`/flows/${id}`]}>
      <Routes>
        <Route path="/flows/:id" element={<FlowCanvasPage />} />
      </Routes>
    </MemoryRouter>
  );
}

describe('FlowCanvasPage', () => {
  beforeEach(() => {
    getFlow.mockReset();
  });

  it('shows a loading state while the flow is being fetched', () => {
    getFlow.mockReturnValue(new Promise(() => {})); // never resolves
    renderAtFlowId('test-id');

    expect(screen.getByText('Loading workflow…')).toBeInTheDocument();
  });

  it('loads the flow and renders the canvas with the flow name as the title', async () => {
    getFlow.mockResolvedValue(makeFlow());
    renderAtFlowId('test-id');

    await waitFor(() => expect(screen.getByTestId('flow-canvas')).toBeInTheDocument());
    expect(getFlow).toHaveBeenCalledWith('test-id');
    expect(screen.getByText('Daily digest')).toBeInTheDocument();
  });

  it('shows a not-found state when the flow does not exist', async () => {
    getFlow.mockRejectedValue(new Error("flow 'missing-id' not found"));
    renderAtFlowId('missing-id');

    await waitFor(() => expect(screen.getByTestId('flow-canvas-not-found')).toBeInTheDocument());
  });

  it('shows an error state for any other failure', async () => {
    getFlow.mockRejectedValue(new Error('core unreachable'));
    renderAtFlowId('test-id');

    await waitFor(() => expect(screen.getByTestId('flow-canvas-error')).toBeInTheDocument());
    expect(screen.getByText('core unreachable')).toBeInTheDocument();
  });

  it('ignores a stale response for a superseded id after navigating to a new one', async () => {
    // Deferred promises so the test controls resolution order precisely: the
    // first (old-id) fetch resolves AFTER the second (new-id) one, mimicking
    // a slow response for a page the user has since navigated away from.
    let resolveFirst!: (flow: Flow) => void;
    const firstFetch = new Promise<Flow>(resolve => {
      resolveFirst = resolve;
    });
    getFlow.mockImplementation((id: string) =>
      id === 'old-id' ? firstFetch : Promise.resolve(makeFlow({ id: 'new-id', name: 'New flow' }))
    );

    const router = createMemoryRouter([{ path: '/flows/:id', element: <FlowCanvasPage /> }], {
      initialEntries: ['/flows/old-id'],
    });
    render(<RouterProvider router={router} />);

    // Navigate away before the old id's fetch resolves.
    router.navigate('/flows/new-id');
    await waitFor(() => expect(screen.getByText('New flow')).toBeInTheDocument());

    // Now let the stale old-id fetch resolve — it must not clobber the
    // already-rendered new-id state.
    resolveFirst(makeFlow({ id: 'old-id', name: 'Old flow (stale)' }));
    await Promise.resolve();
    await Promise.resolve();

    expect(screen.getByText('New flow')).toBeInTheDocument();
    expect(screen.queryByText('Old flow (stale)')).not.toBeInTheDocument();
  });
});
