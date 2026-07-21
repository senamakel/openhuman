import { fireEvent, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { HarnessInitSnapshot } from '../../services/harnessInitService';
import { renderWithProviders } from '../../test/test-utils';
// Imported after the mock is registered.
import HarnessInitOverlay from './HarnessInitOverlay';

// The overlay polls the service; drive it with a controllable mock.
const fetchHarnessInitStatus = vi.fn<() => Promise<HarnessInitSnapshot | null>>();
const runHarnessInit = vi.fn<(force?: boolean) => Promise<HarnessInitSnapshot | null>>();

vi.mock('../../services/harnessInitService', () => ({
  fetchHarnessInitStatus: () => fetchHarnessInitStatus(),
  runHarnessInit: (force?: boolean) => runHarnessInit(force),
}));

function snapshot(overrides: Partial<HarnessInitSnapshot> = {}): HarnessInitSnapshot {
  return {
    overall: 'running',
    startedAt: '2026-07-20T00:00:00Z',
    finishedAt: null,
    steps: [
      {
        id: 'python_runtime',
        label: 'Python runtime',
        required: false,
        state: 'running',
        message: null,
        percent: null,
        updatedAt: null,
      },
    ],
    ...overrides,
  };
}

beforeEach(() => {
  fetchHarnessInitStatus.mockReset();
  runHarnessInit.mockReset();
  window.sessionStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('HarnessInitOverlay', () => {
  it('renders nothing on a warm start (already done)', async () => {
    fetchHarnessInitStatus.mockResolvedValue(snapshot({ overall: 'done', startedAt: 'warm-run' }));

    const { container } = renderWithProviders(<HarnessInitOverlay />);

    await waitFor(() => expect(fetchHarnessInitStatus).toHaveBeenCalled());
    expect(screen.queryByText('Run in background')).not.toBeInTheDocument();
    expect(container).toBeEmptyDOMElement();
  });

  it('shows the blocking overlay while a provisioning run is in progress', async () => {
    fetchHarnessInitStatus.mockResolvedValue(snapshot({ startedAt: 'cold-run' }));

    renderWithProviders(<HarnessInitOverlay />);

    expect(await screen.findByText('Run in background')).toBeInTheDocument();
  });

  it('keeps the overlay dismissed across a remount for the same run (GH-5047)', async () => {
    const run = snapshot({ startedAt: 'same-run' });
    fetchHarnessInitStatus.mockResolvedValue(run);

    const first = renderWithProviders(<HarnessInitOverlay />);
    fireEvent.click(await screen.findByText('Run in background'));
    await waitFor(() => expect(screen.queryByText('Run in background')).not.toBeInTheDocument());
    first.unmount();

    // Remount while the same run is still in progress — it must not reopen.
    const second = renderWithProviders(<HarnessInitOverlay />);
    await waitFor(() => expect(fetchHarnessInitStatus).toHaveBeenCalled());
    // Give any pending poll a chance to (wrongly) re-render the overlay.
    await Promise.resolve();
    expect(screen.queryByText('Run in background')).not.toBeInTheDocument();
    expect(second.container).toBeEmptyDOMElement();
  });

  it('coalesces overlapping status polls into one RPC (StrictMode double-mount)', async () => {
    // Hold the fetch pending so both mounts' immediate polls overlap.
    let resolveFetch: (snap: HarnessInitSnapshot) => void = () => {};
    fetchHarnessInitStatus.mockImplementation(
      () =>
        new Promise<HarnessInitSnapshot>(resolve => {
          resolveFetch = resolve;
        })
    );

    // Two concurrent overlays stand in for the effect→cleanup→effect double-mount.
    renderWithProviders(<HarnessInitOverlay />);
    renderWithProviders(<HarnessInitOverlay />);

    // Both immediate polls should share a single in-flight request.
    expect(fetchHarnessInitStatus).toHaveBeenCalledTimes(1);

    resolveFetch(snapshot({ overall: 'done', startedAt: 'warm-run' }));
    await waitFor(() => expect(screen.queryByText('Run in background')).not.toBeInTheDocument());
  });

  it('reopens for a genuinely new provisioning run after a prior dismissal', async () => {
    // Dismiss the first run.
    fetchHarnessInitStatus.mockResolvedValue(snapshot({ startedAt: 'run-1' }));
    const first = renderWithProviders(<HarnessInitOverlay />);
    fireEvent.click(await screen.findByText('Run in background'));
    await waitFor(() => expect(screen.queryByText('Run in background')).not.toBeInTheDocument());
    first.unmount();

    // A new run (fresh startedAt) is allowed to surface again.
    fetchHarnessInitStatus.mockResolvedValue(snapshot({ startedAt: 'run-2' }));
    renderWithProviders(<HarnessInitOverlay />);
    expect(await screen.findByText('Run in background')).toBeInTheDocument();
  });
});
