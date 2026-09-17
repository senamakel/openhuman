/**
 * Tests for coreHealthMonitor — covers changed lines 17-19, 21-23, 25-29,
 * 31-34, 37, 41-42, 47-50, 53-57, 60-64.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// Mock store and connectivitySlice first.
const dispatchMock = vi.fn();
// Per-test state factory: tests that need a specific `connectivity` slice
// (e.g. a degraded hosted link for the cadence checks) install their own
// implementation; `beforeEach` restores the healthy default so nothing leaks.
const healthyConnectivity = () => ({ connectivity: { core: 'reachable' } });
const getStateMock = vi.fn(healthyConnectivity);
vi.mock('../../store/index', () => ({
  store: { dispatch: dispatchMock, getState: () => getStateMock() },
}));

const setCoreMock = vi.fn((payload: unknown) => ({ type: 'connectivity/setCore', payload }));
const setHostedMock = vi.fn((payload: unknown) => ({ type: 'connectivity/setHosted', payload }));
vi.mock('../../store/connectivitySlice', () => ({
  setCore: (p: unknown) => setCoreMock(p),
  setHosted: (p: unknown) => setHostedMock(p),
}));

const callCoreRpcMock = vi.fn();
vi.mock('../coreRpcClient', () => ({ callCoreRpc: callCoreRpcMock }));

/** Flush all pending microtasks (resolved promises). */
async function flushPromises(): Promise<void> {
  // Multiple rounds handle chained .then() callbacks.
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
  }
}

describe('coreHealthMonitor', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetModules();
    getStateMock.mockReset();
    getStateMock.mockImplementation(healthyConnectivity);
    dispatchMock.mockClear();
    setCoreMock.mockClear();
    setHostedMock.mockClear();
    callCoreRpcMock.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('startCoreHealthMonitor probes immediately on start (lines 53-57)', async () => {
    callCoreRpcMock.mockResolvedValueOnce({});

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();

    // Flush micro-tasks so the async probe runs.
    await flushPromises();

    expect(callCoreRpcMock).toHaveBeenCalledWith(
      expect.objectContaining({ method: 'openhuman.connectivity_diag' })
    );
    stopCoreHealthMonitor();
  });

  it('dispatches reachable on successful probe (lines 25-29)', async () => {
    callCoreRpcMock.mockResolvedValueOnce({});

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    await flushPromises();

    expect(setCoreMock).toHaveBeenCalledWith({ value: 'reachable' });
    stopCoreHealthMonitor();
  });

  it('does not dispatch unreachable until FAIL_THRESHOLD consecutive failures (lines 31-34)', async () => {
    // First failure — below threshold (2), should NOT dispatch unreachable yet.
    callCoreRpcMock.mockRejectedValueOnce(new Error('ECONNREFUSED'));

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    await flushPromises();

    // Only 1 failure, threshold is 2 — unreachable must NOT have been dispatched.
    const unreachableCalls = setCoreMock.mock.calls.filter(
      ([arg]) => (arg as { value: string }).value === 'unreachable'
    );
    expect(unreachableCalls).toHaveLength(0);
    stopCoreHealthMonitor();
  });

  it('dispatches unreachable after FAIL_THRESHOLD consecutive failures (lines 31-34)', async () => {
    // Two consecutive failures → should cross the threshold.
    callCoreRpcMock
      .mockRejectedValueOnce(new Error('ECONNREFUSED first'))
      .mockRejectedValueOnce(new Error('ECONNREFUSED second'));

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();

    // First probe.
    await flushPromises();

    // Advance timer to trigger the degraded-mode 5 s retry.
    vi.advanceTimersByTime(5_001);
    await flushPromises();

    const unreachableCalls = setCoreMock.mock.calls.filter(
      ([arg]) => (arg as { value: string }).value === 'unreachable'
    );
    expect(unreachableCalls.length).toBeGreaterThanOrEqual(1);
    stopCoreHealthMonitor();
  });

  it('is idempotent — second startCoreHealthMonitor call is a no-op (lines 53-54)', async () => {
    callCoreRpcMock.mockResolvedValue({});

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    startCoreHealthMonitor(); // second call must not double-probe

    await flushPromises();

    // Only 1 probe should have fired.
    expect(callCoreRpcMock).toHaveBeenCalledTimes(1);
    stopCoreHealthMonitor();
  });

  it('stopCoreHealthMonitor prevents further scheduling (lines 60-64)', async () => {
    callCoreRpcMock.mockResolvedValue({});

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    await flushPromises();

    const firstCallCount = callCoreRpcMock.mock.calls.length;
    stopCoreHealthMonitor();

    // Advancing time should not trigger another probe.
    vi.advanceTimersByTime(60_000);
    await flushPromises();

    expect(callCoreRpcMock).toHaveBeenCalledTimes(firstCallCount);
  });

  it('schedule picks degraded interval when consecutiveFails > 0 (lines 41-42, 47-50)', async () => {
    // Make probe fail once so consecutiveFails becomes 1.
    callCoreRpcMock.mockRejectedValueOnce(new Error('connection refused')).mockResolvedValue({});

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    await flushPromises();

    // After 1 failure the next poll should be at DEGRADED_INTERVAL_MS = 5s, not 30s.
    vi.advanceTimersByTime(5_001);
    await flushPromises();

    // Second probe should have fired (recovery check).
    expect(callCoreRpcMock).toHaveBeenCalledTimes(2);
    stopCoreHealthMonitor();
  });

  it('error message is extracted from Error instance (lines 31-34)', async () => {
    callCoreRpcMock
      .mockRejectedValueOnce(new Error('timeout msg'))
      .mockRejectedValueOnce(new Error('timeout msg'));

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    await flushPromises();

    vi.advanceTimersByTime(5_001);
    await flushPromises();

    const unreachableCall = setCoreMock.mock.calls.find(
      ([arg]) => (arg as { value: string }).value === 'unreachable'
    );
    expect(unreachableCall).toBeDefined();
    expect((unreachableCall![0] as { error: string }).error).toBe('timeout msg');
    stopCoreHealthMonitor();
  });

  it("mirrors the core's hosted link from the diag reply (#6256)", async () => {
    // Real wire shape: the handler answers `{ diag: … }` and the RPC layer's
    // log envelope wraps it as `{ result, logs }` (#6080).
    callCoreRpcMock.mockResolvedValueOnce({
      result: {
        diag: {
          socket_state: 'reconnecting',
          last_ws_error: 'Ping timeout',
          sidecar_pid: 42,
          listen_port: 7788,
          listen_port_in_use: true,
        },
      },
      logs: ['connectivity diag returned'],
    });

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    await flushPromises();

    expect(setHostedMock).toHaveBeenCalledWith({ value: 'reconnecting', error: 'Ping timeout' });
    stopCoreHealthMonitor();
  });

  it('reports the hosted link as unknown when the diag reply carries no socket state', async () => {
    callCoreRpcMock.mockResolvedValueOnce({});

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    await flushPromises();

    expect(setHostedMock).toHaveBeenCalledWith({ value: 'unknown' });
    stopCoreHealthMonitor();
  });

  it('polls at the degraded cadence while the hosted link is retrying (#6256)', async () => {
    callCoreRpcMock.mockResolvedValue({
      result: { diag: { socket_state: 'reconnecting' } },
      logs: ['connectivity diag returned'],
    });
    // The mocked store never applies the dispatch, so mirror what the reducer
    // would have stored before the monitor picks its next interval.
    getStateMock.mockImplementation(() => ({
      connectivity: { core: 'reachable', hosted: 'reconnecting' },
    }));

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    await flushPromises();
    expect(callCoreRpcMock).toHaveBeenCalledTimes(1);

    // Core is reachable and never failed: without the hosted rule this would
    // be a 30s heartbeat and nothing would fire at 5s.
    vi.advanceTimersByTime(5_001);
    await flushPromises();
    expect(callCoreRpcMock).toHaveBeenCalledTimes(2);
    stopCoreHealthMonitor();
  });

  it('keeps the healthy cadence when the hosted link is simply not running', async () => {
    callCoreRpcMock.mockResolvedValue({
      result: { diag: { socket_state: 'disconnected' } },
      logs: ['connectivity diag returned'],
    });
    getStateMock.mockImplementation(() => ({
      connectivity: { core: 'reachable', hosted: 'unknown' },
    }));

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    await flushPromises();
    expect(setHostedMock).toHaveBeenCalledWith({ value: 'unknown' });

    vi.advanceTimersByTime(5_001);
    await flushPromises();
    expect(callCoreRpcMock).toHaveBeenCalledTimes(1);
    stopCoreHealthMonitor();
  });

  it('error message falls back to String(err) when not an Error instance (lines 31-34)', async () => {
    callCoreRpcMock
      .mockRejectedValueOnce('plain string error')
      .mockRejectedValueOnce('plain string error');

    const { startCoreHealthMonitor, stopCoreHealthMonitor } = await import('../coreHealthMonitor');
    startCoreHealthMonitor();
    await flushPromises();

    vi.advanceTimersByTime(5_001);
    await flushPromises();

    const unreachableCall = setCoreMock.mock.calls.find(
      ([arg]) => (arg as { value: string }).value === 'unreachable'
    );
    expect(unreachableCall).toBeDefined();
    expect((unreachableCall![0] as { error: string }).error).toBe('plain string error');
    stopCoreHealthMonitor();
  });
});

describe('hostedStateFromDiag', () => {
  it('passes the live-loop statuses through verbatim', async () => {
    const { hostedStateFromDiag } = await import('../coreHealthMonitor');
    for (const socket_state of ['connected', 'connecting', 'reconnecting', 'error']) {
      expect(hostedStateFromDiag({ socket_state })).toEqual({ value: socket_state });
      expect(hostedStateFromDiag({ socket_state, socket_loop_active: true })).toEqual({
        value: socket_state,
      });
    }
  });

  it('lets the core decide who is retrying via socket_loop_active (#6256)', async () => {
    const { hostedStateFromDiag } = await import('../coreHealthMonitor');
    // Loop alive + namespace closed by the server: an outage worth showing.
    expect(hostedStateFromDiag({ socket_state: 'disconnected', socket_loop_active: true })).toEqual(
      { value: 'disconnected' }
    );
    // Loop not running: nobody is retrying, whatever the last status said.
    expect(
      hostedStateFromDiag({ socket_state: 'disconnected', socket_loop_active: false })
    ).toEqual({ value: 'unknown' });
    expect(hostedStateFromDiag({ socket_state: 'connected', socket_loop_active: false })).toEqual({
      value: 'unknown',
    });
    // A core that predates the flag: `disconnected` stays `unknown` (legacy inference).
    expect(hostedStateFromDiag({ socket_state: 'disconnected' })).toEqual({ value: 'unknown' });
  });

  it('reports a loop that stopped on an unusable session as stopped, with its reason (#6270)', async () => {
    const { hostedStateFromDiag } = await import('../coreHealthMonitor');
    expect(
      hostedStateFromDiag({
        socket_state: 'disconnected',
        socket_loop_active: false,
        socket_loop_stopped_on_failure: true,
        last_ws_error: 'session expired — please sign in again',
      })
    ).toEqual({ value: 'stopped', error: 'session expired — please sign in again' });
    // A loop that is not running for any other reason is still not an outage.
    expect(
      hostedStateFromDiag({
        socket_state: 'disconnected',
        socket_loop_active: false,
        socket_loop_stopped_on_failure: false,
      })
    ).toEqual({ value: 'unknown' });
    // The failure flag only means something while the loop is not running.
    expect(
      hostedStateFromDiag({
        socket_state: 'connected',
        socket_loop_active: true,
        socket_loop_stopped_on_failure: true,
      })
    ).toEqual({ value: 'connected' });
  });

  it('peels the handler envelope and the log envelope the RPC layer adds (#6080)', async () => {
    const { hostedStateFromDiag } = await import('../coreHealthMonitor');
    // Bare handler answer.
    expect(hostedStateFromDiag({ diag: { socket_state: 'connected' } })).toEqual({
      value: 'connected',
    });
    // What actually crosses the wire today: `single_log` wraps as `{ result, logs }`.
    expect(
      hostedStateFromDiag({
        result: { diag: { socket_state: 'error', last_ws_error: 'boom' } },
        logs: ['connectivity diag returned'],
      })
    ).toEqual({ value: 'error', error: 'boom' });
    // An envelope with nothing usable inside is still `unknown`, never a throw.
    expect(hostedStateFromDiag({ result: null, logs: [] })).toEqual({ value: 'unknown' });
    expect(hostedStateFromDiag({ result: { diag: 'nope' } })).toEqual({ value: 'unknown' });
  });

  it('collapses a loop that is not running, and anything unrecognised, to unknown', async () => {
    const { hostedStateFromDiag } = await import('../coreHealthMonitor');
    expect(hostedStateFromDiag({ socket_state: 'disconnected' })).toEqual({ value: 'unknown' });
    expect(hostedStateFromDiag({ socket_state: 'uninitialized' })).toEqual({ value: 'unknown' });
    expect(hostedStateFromDiag({ socket_state: 42 })).toEqual({ value: 'unknown' });
    expect(hostedStateFromDiag({})).toEqual({ value: 'unknown' });
    expect(hostedStateFromDiag(null)).toEqual({ value: 'unknown' });
    expect(hostedStateFromDiag('nope')).toEqual({ value: 'unknown' });
  });

  it('carries a non-empty last_ws_error and drops an empty or non-string one', async () => {
    const { hostedStateFromDiag } = await import('../coreHealthMonitor');
    expect(hostedStateFromDiag({ socket_state: 'error', last_ws_error: 'boom' })).toEqual({
      value: 'error',
      error: 'boom',
    });
    expect(hostedStateFromDiag({ socket_state: 'connected', last_ws_error: '' })).toEqual({
      value: 'connected',
    });
    expect(hostedStateFromDiag({ socket_state: 'connected', last_ws_error: null })).toEqual({
      value: 'connected',
    });
  });
});
