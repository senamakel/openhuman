/**
 * E2E: Differentiate device offline, backend unreachable, socket disconnected,
 * and core offline states (issue #1527).
 *
 * Verifies that the UI shows distinct status copy and actions for each
 * connectivity failure mode, and that recovery transitions work without
 * requiring a reinstall or data reset.
 *
 * ## Driver notes
 * - Backend-unreachable: `setMockBehavior('forceHttpStatus', '503')` makes
 *   every non-admin route return 503, which drops the Socket.IO handshake and
 *   triggers `backend-only` state. Cleared by resetting to '' restores.
 * - Socket-disconnected: POST to `/__admin/socket/disconnect` closes all
 *   active Socket.IO sessions server-side. The client reconnect loop then
 *   surfaces `backend-only` copy.
 * - Internet-offline: simulated via `window.dispatchEvent(new Event('offline'))`
 *   in the WebView. Triggers the `internet-offline` branch in connectivitySlice.
 * - Core-offline: the embedded core runs in-process inside the Tauri host and
 *   cannot be stopped without killing the entire app process. There is a
 *   `restart_core_process` Tauri command, but no Tauri command to *stop* the
 *   core without immediately restarting it, and no way to invoke Tauri commands
 *   from outside the WebView renderer during E2E. Scenario is skipped with a
 *   TODO; see product gap note below.
 *
 * ## Product gap
 * There is no Tauri IPC command exposed to E2E that stops the embedded core
 * without restarting it. `restart_core_process` bounces the core but returns
 * only after the core is healthy again, so it cannot simulate an offline
 * window. A future `stop_core_process` Tauri command (or a debug-build-only
 * RPC hook) would unblock the core-offline scenario. See issue #1527.
 */
import { waitForApp, waitForAppReady } from '../helpers/app-helpers';
import { resetApp } from '../helpers/reset-app';
import {
  textExists,
  waitForText,
} from '../helpers/element-helpers';
import {
  resetMockBehavior,
  setMockBehavior,
  startMockServer,
  stopMockServer,
} from '../mock-server';

const USER_ID = 'e2e-connectivity-state-differentiation';

/** Stable text fragments from en.ts for each blocking state. */
const STATUS_TEXT = {
  internetOffline: "Your device is offline right now",
  coreUnreachable: "The OpenHuman core isn't responding",
  backendOnly: "Reconnecting to backend",
  reconnecting: "Reconnecting",
  coreOffline: "Core offline",
  offline: "Offline",
} as const;

/** Timeout for connectivity state changes to propagate to the UI. */
const CONNECTIVITY_SETTLE_MS = 12_000;

function stepLog(message: string): void {
  console.log(`[ConnectivityDiffE2E][${new Date().toISOString()}] ${message}`);
}

/**
 * Call the mock admin endpoint directly from Node (outside the WebView) to
 * disconnect all Socket.IO clients. Returns the number of sessions
 * disconnected, or -1 on failure.
 */
async function adminDisconnectSockets(): Promise<number> {
  const { getMockServerPort } = await import('../../../scripts/mock-api-core.mjs');
  const port = getMockServerPort();
  stepLog(`Posting to /__admin/socket/disconnect on mock port ${port}`);
  try {
    const res = await fetch(`http://127.0.0.1:${port}/__admin/socket/disconnect`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({}),
    });
    const json = (await res.json()) as { success?: boolean; data?: { disconnected?: number } };
    const count = json.data?.disconnected ?? 0;
    stepLog(`adminDisconnectSockets: disconnected=${count}`);
    return count;
  } catch (err) {
    stepLog(`adminDisconnectSockets failed: ${err}`);
    return -1;
  }
}

/**
 * Simulate device-offline inside the WebView by dispatching the native
 * 'offline' DOM event. The connectivity slice listens on window.
 */
async function simulateDeviceOffline(): Promise<void> {
  await browser.execute(() => {
    window.dispatchEvent(new Event('offline'));
  });
}

/**
 * Restore device-online inside the WebView by dispatching the native
 * 'online' DOM event.
 */
async function simulateDeviceOnline(): Promise<void> {
  await browser.execute(() => {
    window.dispatchEvent(new Event('online'));
  });
}

describe('Connectivity state differentiation (issue #1527)', () => {
  before(async function beforeSuite() {
    this.timeout(120_000);
    stepLog('Starting mock server');
    await startMockServer();
    stepLog('Waiting for app');
    await waitForApp();
    stepLog('Resetting app state');
    await resetApp(USER_ID);
    stepLog('Suite setup complete');
  });

  afterEach(async () => {
    // Always restore clean mock behavior and online state after each test so
    // subsequent scenarios start from a known baseline.
    resetMockBehavior();
    try {
      await simulateDeviceOnline();
    } catch {
      // Non-fatal — if the WebView is in a bad state the next reset will fix it.
    }
  });

  after(async () => {
    stepLog('Stopping mock server');
    await stopMockServer();
  });

  // ---------------------------------------------------------------------------
  // Scenario 1: Internet available, backend unreachable
  // ---------------------------------------------------------------------------
  it('shows backend-reconnecting status when backend is unreachable but internet is up', async function () {
    this.timeout(60_000);
    stepLog('Injecting 503 on all backend routes to simulate backend unreachable');

    // Force every non-admin HTTP route to return 503. This drops the
    // Socket.IO handshake and causes socketService to enter the
    // 'disconnected'/'connecting' cycle, surfacing 'backend-only' state.
    setMockBehavior('forceHttpStatus', '503');

    stepLog('Waiting for backend-reconnecting copy to appear');
    await waitForText(STATUS_TEXT.backendOnly, CONNECTIVITY_SETTLE_MS);

    // Device should NOT show a generic device-offline indicator — internet
    // is still up and the copy is backend-specific.
    const showsDeviceOffline = await textExists(STATUS_TEXT.internetOffline);
    expect(showsDeviceOffline).toBe(
      false,
      'Expected no device-offline copy when only backend is unreachable'
    );

    stepLog('Restoring backend connectivity');
    resetMockBehavior();

    // Give the client time to reconnect and surface the healthy state.
    await browser.waitUntil(
      async () => {
        const stillReconnecting = await textExists(STATUS_TEXT.backendOnly);
        return !stillReconnecting;
      },
      {
        timeout: CONNECTIVITY_SETTLE_MS,
        interval: 1_000,
        timeoutMsg: 'Backend-reconnecting banner did not clear after mock backend recovered',
      }
    );
    stepLog('Backend reconnected — banner cleared');
  });

  // ---------------------------------------------------------------------------
  // Scenario 2: Socket disconnected (backend reachable, socket layer dropped)
  // ---------------------------------------------------------------------------
  it('shows reconnecting status after socket is force-disconnected server-side', async function () {
    this.timeout(60_000);
    stepLog('Force-disconnecting all socket sessions via admin endpoint');

    const disconnected = await adminDisconnectSockets();
    stepLog(`Server-side socket sessions disconnected: ${disconnected}`);

    // After a server-side socket disconnect the client's Socket.IO library
    // starts its reconnect loop and the UI should transition to the
    // backend-only / reconnecting state. It should NOT show device-offline copy.
    stepLog('Waiting for reconnecting copy to appear');
    await waitForText(STATUS_TEXT.reconnecting, CONNECTIVITY_SETTLE_MS);

    const showsDeviceOffline = await textExists(STATUS_TEXT.internetOffline);
    expect(showsDeviceOffline).toBe(
      false,
      'Expected no device-offline copy when only socket is disconnected'
    );

    // The Socket.IO client reconnects automatically — the banner should clear.
    stepLog('Waiting for automatic socket reconnect');
    await browser.waitUntil(
      async () => {
        const stillReconnecting = await textExists(STATUS_TEXT.reconnecting);
        return !stillReconnecting;
      },
      {
        timeout: CONNECTIVITY_SETTLE_MS,
        interval: 1_000,
        timeoutMsg: 'Reconnecting banner did not clear after socket reconnected',
      }
    );
    stepLog('Socket reconnected — banner cleared');
  });

  // ---------------------------------------------------------------------------
  // Scenario 3: True device offline
  // ---------------------------------------------------------------------------
  it('shows device-offline copy (not backend-only) when window fires "offline" event', async function () {
    this.timeout(30_000);
    stepLog('Simulating device offline event in WebView');
    await simulateDeviceOffline();

    stepLog('Waiting for device-offline copy to appear');
    await waitForText(STATUS_TEXT.internetOffline, CONNECTIVITY_SETTLE_MS);

    // Should show internet-offline copy, NOT backend-only reconnecting copy.
    const showsBackendOnly = await textExists(STATUS_TEXT.backendOnly);
    expect(showsBackendOnly).toBe(
      false,
      'Expected no backend-reconnecting copy when device is fully offline'
    );

    stepLog('Restoring device online');
    await simulateDeviceOnline();

    // The app should stop showing device-offline copy once internet is restored.
    await browser.waitUntil(
      async () => {
        const stillOffline = await textExists(STATUS_TEXT.internetOffline);
        return !stillOffline;
      },
      {
        timeout: CONNECTIVITY_SETTLE_MS,
        interval: 500,
        timeoutMsg: 'Device-offline copy did not clear after online event was dispatched',
      }
    );
    stepLog('Device online — device-offline copy cleared');
  });

  // ---------------------------------------------------------------------------
  // Scenario 4: Backend recovers after 503 — no reinstall/data-reset required
  // ---------------------------------------------------------------------------
  it('status updates to healthy without reinstall after backend recovers from 503', async function () {
    this.timeout(60_000);
    stepLog('Injecting 503 to drop backend connectivity');
    setMockBehavior('forceHttpStatus', '503');

    await waitForText(STATUS_TEXT.backendOnly, CONNECTIVITY_SETTLE_MS);
    stepLog('Backend-only state confirmed; restoring mock backend');

    resetMockBehavior();

    // The app must recover by itself — verify we do NOT need any user-initiated
    // reinstall or data-reset flow to clear the error state.
    await browser.waitUntil(
      async () => {
        const stillReconnecting = await textExists(STATUS_TEXT.backendOnly);
        return !stillReconnecting;
      },
      {
        timeout: CONNECTIVITY_SETTLE_MS,
        interval: 1_000,
        timeoutMsg: 'App did not self-recover from backend-unreachable without reinstall',
      }
    );
    stepLog('Status recovered automatically — no reinstall needed');
  });

  // ---------------------------------------------------------------------------
  // Scenario 5: Internet available + core offline → core-specific indicator
  //
  // SKIPPED: The embedded core runs in-process inside the Tauri host. There
  // is no Tauri IPC command accessible from the E2E harness that stops the
  // core without immediately restarting it. `restart_core_process` bounces
  // the core but only returns after it is healthy again, so there is no
  // observable window where the UI can show the `core-unreachable` state.
  //
  // Product gap: expose a `stop_core_process` Tauri command (debug-build-only
  // is acceptable) so the test harness can drive the `core-unreachable` branch
  // and assert that the UI shows "Core offline" rather than "Offline" (the
  // device-offline copy). Tracked in issue #1527.
  // ---------------------------------------------------------------------------
  it.skip('shows core-offline indicator (not device-offline) when internet is up but core is unreachable', async () => {
    // TODO(issue #1527): implement once a `stop_core_process` or equivalent
    // debug Tauri command exists. Steps:
    //   1. Invoke `stop_core_process` via browser.execute + window.__TAURI_INTERNALS__
    //      (requires debug build with the command registered).
    //   2. Wait for the core health-monitor poll to fire and update connectivity.core.
    //   3. Assert `textExists('Core offline')` === true.
    //   4. Assert `textExists('Offline')` === false (not device-offline copy).
    //   5. Assert `textExists("The OpenHuman core isn't responding")` === true.
    //   6. Restart the core and assert the indicator recovers.
    await waitForAppReady(5_000);
  });
});
