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
// Note: we deliberately do NOT use shared-flows' completeOnboardingIfVisible
// here — that walker matches a hardcoded list of step *titles* (Welcome,
// Install Skills, …) and silently skips when the onboarding flow grows a
// new step (e.g. "Connect your Gmail"). We key off the onboarding-next-button
// testid instead, which is stable across steps.

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
 * The Tauri shell's BootCheckGate shows a "Choose core mode" modal whenever
 * Redux `coreMode.kind === 'unset'`. Click the primary "Continue" CTA to
 * confirm bundled-core mode. We dispatch a real synthetic MouseEvent so
 * React's onClick handler picks it up reliably, and retry until the modal
 * is gone (a single click can race against the gate's re-render).
 */
async function dismissCoreModeModalIfVisible(timeoutMs = 15_000): Promise<boolean> {
  if (!supportsExecuteScript()) return false;
  const deadline = Date.now() + timeoutMs;
  let everSeen = false;
  while (Date.now() < deadline) {
    const status = await browser.execute(() => {
      const heading = Array.from(document.querySelectorAll('h2')).find(
        h => (h.textContent ?? '').trim() === 'Choose core mode'
      );
      if (!heading) return 'gone';
      const modal = heading.closest('.fixed') ?? heading.parentElement;
      if (!modal) return 'gone';
      const buttons = Array.from(modal.querySelectorAll<HTMLButtonElement>('button'));
      const primary =
        buttons.find(b => (b.textContent ?? '').trim() === 'Continue') ??
        buttons.find(b => /bg-ocean-500/.test(b.className)) ??
        buttons[buttons.length - 1];
      if (!primary) return 'visible-no-button';
      ['mousedown', 'mouseup', 'click'].forEach(type => {
        primary.dispatchEvent(
          new MouseEvent(type, { bubbles: true, cancelable: true, view: window, button: 0 })
        );
      });
      return 'clicked';
    });
    if (status === 'gone') return everSeen;
    everSeen = true;
    await browser.pause(800);
  }
  stepLog('"Choose core mode" modal never dismissed within budget');
  return everSeen;
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
  // The sidecar only spawns after the first successful user login, so the
  // very first spec of a run hits an unreachable RPC — that's not an error,
  // a freshly-launched workspace is already in the same "pristine" state
  // the wipe would have produced. Race the RPC call against a short budget
  // and treat the result as a flag: did we actually wipe anything?
  const reset = await Promise.race([
    callOpenhumanRpc('openhuman.test_reset', {}),
    new Promise<{ ok: false; error: string }>(resolve =>
      setTimeout(
        () => resolve({ ok: false, error: 'test_reset RPC probe timed out (sidecar likely not started)' }),
        8_000
      )
    ),
  ]);
  let didWipe = false;
  if (reset.ok) {
    stepLog(`Sidecar wipe ok: ${JSON.stringify(reset.result)}`);
    didWipe = true;
  } else {
    const errText = String(reset.error ?? '');
    const unreachable =
      errText.includes('not reachable') ||
      errText.includes('probe timed out') ||
      errText.includes('ECONNREFUSED');
    if (!unreachable) {
      throw new Error(`openhuman.test_reset failed: ${errText || JSON.stringify(reset)}`);
    }
    stepLog(`Sidecar not reachable (${errText}) — treating as fresh launch, skipping wipe`);
  }

  // Only reload the renderer when we actually wiped state — otherwise we
  // throw away the in-app "Choose core mode" acceptance the app shell has
  // already cleared, and end up wedged behind that modal on first launch.
  if (didWipe && supportsExecuteScript()) {
    stepLog('Clearing renderer storage + reloading webview');
    await browser.execute(() => {
      try {
        window.localStorage.clear();
        window.sessionStorage.clear();
      } catch (err) {
        console.warn('[resetApp] storage.clear failed', err);
      }
      window.location.replace('#/');
      window.location.reload();
    });
  } else if (didWipe) {
    stepLog('execute() unsupported — skipping renderer reload (state may be stale)');
  } else {
    stepLog('Skipping renderer reload — nothing was wiped');
  }

  await waitForApp();
  await waitForWindowVisible(25_000);
  await waitForWebView(15_000);
  await waitForAppReady(15_000);
  await dismissCoreModeModalIfVisible();

  if (options.skipAuth) {
    stepLog('skipAuth=true — stopping before auth bypass');
    return userId;
  }

  stepLog(`Triggering auth deep-link bypass for ${userId}`);
  await triggerAuthDeepLinkBypass(userId);
  await waitForAppReady(15_000);
  // BootCheckGate may re-mount after the deep-link routes to /home; dismiss
  // the modal again if it slid back into view.
  await dismissCoreModeModalIfVisible(8_000);
  await walkOnboardingViaTestId(logPrefix);

  stepLog('Reset + onboarding complete');
  return userId;
}

/**
 * Click the onboarding "Continue" button repeatedly until it disappears.
 *
 * The button has `data-testid="onboarding-next-button"` on every step
 * (see app/src/pages/onboarding/components/OnboardingNextButton.tsx), so
 * we don't need to know the step *content* — we just keep advancing while
 * the testid is mounted. Capped at 12 iterations to avoid runaway clicks
 * if a step ever wedges.
 */
async function walkOnboardingViaTestId(logPrefix: string, maxSteps = 12): Promise<number> {
  if (!supportsExecuteScript()) return 0;

  // First, wait up to 15s for the onboarding shell to mount at all. If it
  // never appears the user is already on /home (e.g. onboarding pre-completed
  // for this user id) and we just return.
  const appeared = await browser.waitUntil(
    async () =>
      Boolean(
        await browser.execute(
          () => document.querySelector('[data-testid="onboarding-next-button"]') !== null
        )
      ),
    { timeout: 15_000, interval: 500, timeoutMsg: 'onboarding-next-button never appeared' }
  ).catch(() => false);

  if (!appeared) {
    stepLog(`${logPrefix} Onboarding next-button never appeared — assuming already onboarded`);
    return 0;
  }

  let steps = 0;
  for (let i = 0; i < maxSteps; i += 1) {
    const stillMounted = await browser.execute(() => {
      const btn = document.querySelector<HTMLButtonElement>(
        '[data-testid="onboarding-next-button"]'
      );
      if (!btn) return false;
      if (btn.disabled) return 'disabled';
      ['mousedown', 'mouseup', 'click'].forEach(type => {
        btn.dispatchEvent(
          new MouseEvent(type, { bubbles: true, cancelable: true, view: window, button: 0 })
        );
      });
      return true;
    });

    if (stillMounted === false) {
      stepLog(`${logPrefix} Onboarding finished after ${steps} step(s)`);
      return steps;
    }
    if (stillMounted === 'disabled') {
      // Some steps gate the button on async work (e.g. fetching skills).
      // Give it a moment, then re-check; if it stays disabled the step
      // is in a permanently-stuck state and we bail out.
      stepLog(`${logPrefix} Next-button disabled at step ${steps} — waiting for it to enable`);
      await browser.pause(2_000);
      continue;
    }
    steps += 1;
    // Most steps animate in/out — wait a beat for the next one to render.
    await browser.pause(steps >= 4 ? 3_000 : 1_500);
  }
  stepLog(`${logPrefix} Hit max onboarding steps (${maxSteps}) — moving on`);
  return steps;
}
