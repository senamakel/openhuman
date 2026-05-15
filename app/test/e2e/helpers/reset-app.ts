/**
 * E2E reset — bring the running app back to a fresh-install baseline.
 *
 * We keep ONE Appium session across the whole spec run (see wdio.conf.ts).
 * Instead of restarting the sidecar between specs we call the in-place
 * `openhuman.test_reset` RPC: it wipes the auth marker, clears the
 * `chat_onboarding_completed` flag, removes all cron jobs and saves the
 * config back. The renderer is then reloaded so Redux/localStorage are
 * also flushed, and the spec re-authenticates via the deep-link bypass
 * and walks onboarding through the real UI.
 *
 * Use this as the FIRST thing every spec does:
 *
 *   before(async () => {
 *     await resetApp('e2e-<spec-name>');
 *   });
 *
 * Picking a unique `userId` per spec is what gives each spec its own
 * mock-backend identity so request logs aren't cross-contaminated.
 */
import { waitForApp, waitForAppReady } from './app-helpers';
import { callOpenhumanRpc } from './core-rpc';
import { triggerAuthDeepLinkBypass } from './deep-link-helpers';
import { waitForWebView, waitForWindowVisible } from './element-helpers';
import { supportsExecuteScript } from './platform';
import { completeOnboardingIfVisible } from './shared-flows';

interface ResetAppOptions {
  /** Skip the auth + onboarding bootstrap. Use for specs that test the welcome/login screens themselves. */
  skipAuth?: boolean;
  /** Override the onboarding-walker log prefix. */
  logPrefix?: string;
}

function stepLog(message: string): void {
  console.log(`[resetApp][${new Date().toISOString()}] ${message}`);
}

/**
 * Wipe sidecar + renderer state and (by default) re-auth + onboard.
 *
 * Order matters:
 *   1. RPC wipe — must come BEFORE the reload, otherwise the renderer will
 *      re-hydrate Redux from persisted storage and immediately call out to
 *      a sidecar that still thinks the user is logged in.
 *   2. Clear web storage + reload — the renderer's persisted Redux slices
 *      live in localStorage; if we don't drop them the welcome screen is
 *      skipped and we land back on /home with stale state.
 *   3. Wait for the app to remount.
 *   4. Re-auth via deep-link bypass + walk onboarding through the UI.
 *
 * Returns the userId that was authenticated (mirrors what the spec passed
 * in) so callers can use it for mock-backend request-log assertions.
 */
export async function resetApp(
  userId: string,
  options: ResetAppOptions = {}
): Promise<string> {
  const logPrefix = options.logPrefix ?? '[resetApp]';

  stepLog(`Calling openhuman.test_reset for ${userId}`);
  const reset = await callOpenhumanRpc('openhuman.test_reset', {});
  if (!reset.ok) {
    throw new Error(
      `openhuman.test_reset failed: ${JSON.stringify(reset.error ?? reset)}`
    );
  }
  stepLog(`Sidecar wipe ok: ${JSON.stringify(reset.result)}`);

  if (supportsExecuteScript()) {
    stepLog('Clearing renderer storage + reloading webview');
    await browser.execute(() => {
      try {
        window.localStorage.clear();
        window.sessionStorage.clear();
      } catch (err) {
        // Some embedded origins block storage access — best-effort.
        console.warn('[resetApp] storage.clear failed', err);
      }
      window.location.replace('#/');
      window.location.reload();
    });
  } else {
    stepLog('execute() unsupported — skipping renderer reload');
  }

  await waitForApp();
  await waitForWindowVisible(25_000);
  await waitForWebView(15_000);
  await waitForAppReady(15_000);

  if (options.skipAuth) {
    stepLog('skipAuth=true — stopping before auth bypass');
    return userId;
  }

  stepLog(`Triggering auth deep-link bypass for ${userId}`);
  await triggerAuthDeepLinkBypass(userId);
  await waitForAppReady(15_000);
  await completeOnboardingIfVisible(logPrefix);

  stepLog('Reset + onboarding complete');
  return userId;
}
