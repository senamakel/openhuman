import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { setDaemonStatus, updateHealthSnapshot } from '../../features/daemon/store';
import { DaemonHealthService } from '../daemonHealthService';

vi.mock('../../features/daemon/store', () => ({
  setDaemonStatus: vi.fn(),
  updateHealthSnapshot: vi.fn(),
}));

vi.mock('../../lib/coreState/store', () => ({
  getCoreStateSnapshot: () => ({ snapshot: { sessionToken: null } }),
}));

const mockedUpdate = vi.mocked(updateHealthSnapshot);
const mockedSetStatus = vi.mocked(setDaemonStatus);

const healthPayload = (overrides: Record<string, unknown> = {}) => ({
  pid: 123,
  updated_at: '2026-07-21T00:00:00Z',
  uptime_seconds: 10,
  components: { gateway: { status: 'ok', updated_at: '2026-07-21T00:00:00Z', restart_count: 0 } },
  ...overrides,
});

describe('DaemonHealthService.ingestHealthSnapshot', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockedUpdate.mockReset();
    mockedSetStatus.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('parses a valid payload and updates the daemon store', () => {
    const service = new DaemonHealthService();
    service.ingestHealthSnapshot(healthPayload());

    expect(mockedUpdate).toHaveBeenCalledTimes(1);
    const [, snapshot] = mockedUpdate.mock.calls[0];
    expect(snapshot.pid).toBe(123);
    expect(snapshot.components.gateway.status).toBe('ok');

    service.cleanup();
  });

  it('ignores a missing or unparseable payload (older core, no health folded in)', () => {
    const service = new DaemonHealthService();
    service.ingestHealthSnapshot(undefined);
    service.ingestHealthSnapshot(null);
    service.ingestHealthSnapshot({ not: 'a health snapshot' });

    expect(mockedUpdate).not.toHaveBeenCalled();
    service.cleanup();
  });

  it('marks the daemon disconnected when no snapshot arrives within the timeout', () => {
    const service = new DaemonHealthService();
    service.ingestHealthSnapshot(healthPayload());
    expect(mockedSetStatus).not.toHaveBeenCalled();

    // No further ingest for the watchdog window → disconnected.
    vi.advanceTimersByTime(30000);
    expect(mockedSetStatus).toHaveBeenCalledWith(expect.any(String), 'disconnected');

    service.cleanup();
  });

  it('re-arms the disconnect watchdog on each ingest', () => {
    const service = new DaemonHealthService();
    service.ingestHealthSnapshot(healthPayload());

    // A fresh snapshot just before the deadline pushes it out.
    vi.advanceTimersByTime(25000);
    service.ingestHealthSnapshot(healthPayload());
    vi.advanceTimersByTime(25000);
    expect(mockedSetStatus).not.toHaveBeenCalled();

    // Then go quiet past the window → disconnected.
    vi.advanceTimersByTime(30000);
    expect(mockedSetStatus).toHaveBeenCalledWith(expect.any(String), 'disconnected');

    service.cleanup();
  });
});
