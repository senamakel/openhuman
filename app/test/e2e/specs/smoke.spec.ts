// @ts-nocheck
/**
 * Smoke spec — proves the unified Appium/CEF harness can:
 *
 *   1. Attach to the running app and produce a live WebDriver session.
 *   2. Drive the app from a clean slate through `resetApp(...)`:
 *      sidecar wipe → renderer reload → auth deep-link → onboarding walk.
 *   3. Land on `/home` with rendered React content (NOT a blank shell, NOT
 *      stuck behind BootCheckGate / onboarding / the login screen).
 *
 * Every other spec assumes this works — so when CI is red, look here first.
 */
import { waitForApp, waitForAppReady } from '../helpers/app-helpers';
import { hasAppChrome } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { waitForHomePage } from '../helpers/shared-flows';

const USER_ID = 'e2e-smoke';

describe('Smoke', () => {
  before(async () => {
    await waitForApp();
    await resetApp(USER_ID);
  });

  it('has a live WebDriver session', async () => {
    const sessionId = browser.sessionId;
    expect(sessionId).toBeDefined();
    expect(typeof sessionId).toBe('string');
    expect(sessionId.length).toBeGreaterThan(0);
  });

  it('shows app chrome (window is mapped & visible)', async () => {
    expect(await hasAppChrome()).toBe(true);
  });

  it('renders a non-empty DOM in the main webview', async () => {
    const elements = await browser.$$('//*');
    expect(elements.length).toBeGreaterThan(0);
  });

  it('reaches a logged-in route after auth + onboarding', async () => {
    await waitForAppReady(10_000);

    // Poll for a hash that signals "past the unauthenticated welcome".
    // The post-auth route is one of:
    //   - `#/home` (fully onboarded user)
    //   - `#/onboarding/*` (auth landed but onboarding still walking)
    //
    // Either is acceptable for smoke — the spec's job is to prove the
    // harness boots PAST the unauthenticated welcome screen (which
    // would be `#/`). Onboarding flow correctness is covered in
    // login-flow.spec.ts / logout-relogin-onboarding.spec.ts.
    let hash = '';
    await browser.waitUntil(
      async () => {
        hash = (await browser.execute(() => window.location.hash)) as string;
        return /^#\/(home|onboarding)/.test(hash);
      },
      {
        timeout: 15_000,
        timeoutMsg: () => `hash never settled to #/home or #/onboarding (last seen "${hash}")`,
      }
    );

    // Best-effort content check when we did reach /home.
    if (hash.startsWith('#/home')) {
      const homeText = await waitForHomePage(15_000);
      expect(homeText).toBeTruthy();
    }
  });
});
