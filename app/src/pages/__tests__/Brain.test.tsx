import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import Brain from '../Brain';

const graphExportMock = vi.hoisted(() => vi.fn());

vi.mock('../../utils/tauriCommands', () => ({ memoryTreeGraphExport: graphExportMock }));

vi.mock('../../components/intelligence/MemoryGraph', async () => {
  const React = await import('react');
  return {
    MemoryGraph: ({ nodes }: { nodes: unknown[] }) =>
      React.createElement('div', { 'data-testid': 'memory-graph' }, `nodes:${nodes.length}`),
  };
});

vi.mock('../../components/intelligence/MemoryControls', () => ({ MemoryControls: () => null }));
vi.mock('../../components/intelligence/MemoryTreeStatusPanel', () => ({
  MemoryTreeStatusPanel: () => null,
}));
vi.mock('../../components/intelligence/MemorySourcesRegistry', () => ({
  MemorySourcesRegistry: () => null,
}));
vi.mock('../../components/intelligence/Toast', () => ({ ToastContainer: () => null }));

const makeGraph = (n: number) => ({
  nodes: Array.from({ length: n }, (_, i) => ({ id: `n${i}`, kind: 'summary', label: `N${i}` })),
  edges: [],
  content_root_abs: '/tmp/content',
});

describe('Brain page', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the graph once data is fetched', async () => {
    graphExportMock.mockResolvedValue(makeGraph(3));
    render(<Brain />);

    await act(async () => {
      await vi.waitFor(() => {
        expect(screen.getByTestId('memory-graph')).toHaveTextContent('nodes:3');
      });
    });
  });

  it('renders empty-state graph when there are no nodes', async () => {
    graphExportMock.mockResolvedValue(makeGraph(0));
    render(<Brain />);

    await act(async () => {
      await vi.waitFor(() => {
        expect(screen.getByTestId('memory-graph')).toHaveTextContent('nodes:0');
      });
    });
  });

  it('surfaces an error alert when the fetch fails', async () => {
    graphExportMock.mockRejectedValue(new Error('boom'));
    render(<Brain />);

    await act(async () => {
      await vi.waitFor(() => {
        expect(screen.getByRole('alert')).toBeInTheDocument();
      });
    });
  });
});
