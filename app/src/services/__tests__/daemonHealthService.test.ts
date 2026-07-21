import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { callCoreRpc } from '../coreRpcClient';
import { DaemonHealthService } from '../daemonHealthService';

vi.mock('../coreRpcClient', () => ({
  callCoreRpc: vi.fn(),
}));

vi.mock('../../features/daemon/store', () => ({
  setDaemonStatus: vi.fn(),
  updateHealthSnapshot: vi.fn(),
}));

vi.mock('../../lib/coreState/store', () => ({
  getCoreStateSnapshot: () => ({ snapshot: { sessionToken: null } }),
}));

const mockedCallCoreRpc = vi.mocked(callCoreRpc);

const healthPayload = () => ({
  pid: 123,
  updated_at: '2026-07-21T00:00:00Z',
  uptime_seconds: 10,
  components: {},
});

describe('DaemonHealthService', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockedCallCoreRpc.mockReset();
    mockedCallCoreRpc.mockResolvedValue(healthPayload());
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  const countHealthCalls = () =>
    mockedCallCoreRpc.mock.calls.filter(
      ([arg]) => (arg as { method?: string })?.method === 'openhuman.health_snapshot'
    ).length;

  it('shares a single poll loop across concurrent consumers', async () => {
    const service = new DaemonHealthService();

    // Three consumers set up on the same tick (as SocketProvider +
    // ServiceBlockingGate do on startup). Previously each awaited the first
    // poll before assigning the interval id, so all three raced past the guard
    // and spawned their own interval → duplicate health_snapshot RPCs per tick.
    const releases = await Promise.all([
      service.setupHealthListener(),
      service.setupHealthListener(),
      service.setupHealthListener(),
    ]);

    // Exactly one immediate poll, not one-per-consumer.
    expect(countHealthCalls()).toBe(1);

    await vi.advanceTimersByTimeAsync(2000);
    // One interval tick → one additional poll (total 2), not three.
    expect(countHealthCalls()).toBe(2);

    // Releasing one of several consumers must NOT stop polling for the rest.
    releases[0]();
    await vi.advanceTimersByTimeAsync(2000);
    expect(countHealthCalls()).toBe(3);

    // Once the last consumer releases, polling stops.
    releases[1]();
    releases[2]();
    await vi.advanceTimersByTimeAsync(6000);
    expect(countHealthCalls()).toBe(3);

    service.cleanup();
  });

  it('re-arms the poll loop after all consumers release and a new one attaches', async () => {
    const service = new DaemonHealthService();

    const release = await service.setupHealthListener();
    expect(countHealthCalls()).toBe(1);
    release();

    const release2 = await service.setupHealthListener();
    await vi.advanceTimersByTimeAsync(2000);
    // New consumer starts a fresh loop: immediate poll + one tick.
    expect(countHealthCalls()).toBe(3);

    release2();
    service.cleanup();
  });
});
