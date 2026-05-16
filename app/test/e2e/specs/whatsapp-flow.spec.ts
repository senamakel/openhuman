// @ts-nocheck
/**
 * Smoke spec for the WhatsApp Web account integration (feature 10.1.2).
 *
 * Proves the Accounts page exposes WhatsApp Web as an addable provider,
 * the Add Account modal lists it with the expected label, and selecting
 * it routes the UI into the webview-host pane.
 *
 * Deferred to follow-up PRs:
 *   - Real WhatsApp QR-code login (Stage B in #968 / cross-channel epic)
 *   - Inbound message sync assertions (10.3.x)
 *   - Send / reply happy paths (10.4.x)
 *
 * Welcome lockdown (#883) hides the Accounts rail until onboarding
 * completes — `resetApp()` walks onboarding so /accounts is reachable.
 */
import { waitForApp } from '../helpers/app-helpers';
import { clickButton, textExists, waitForText } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash, openAddAccountModal } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-whatsapp-flow';

describe('WhatsApp account integration smoke', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('shows WhatsApp Web as an addable provider in the Add Account modal', async () => {
    await navigateViaHash('/accounts');
    await waitForText('Add app', 15_000);

    await openAddAccountModal();

    await waitForText('WhatsApp Web', 10_000);
    expect(await textExists('WhatsApp Web')).toBe(true);
    expect(await textExists('Open web.whatsapp.com inside the app and stream chat updates.')).toBe(
      true
    );
  });

  it('selecting WhatsApp Web closes the modal and registers an account on the rail', async () => {
    await navigateViaHash('/accounts');
    await waitForText('Add app', 15_000);
    await openAddAccountModal();
    await waitForText('WhatsApp Web', 10_000);

    await clickButton('WhatsApp Web');

    // 1) Modal must close.
    await browser.waitUntil(async () => !(await textExists('Add account')), {
      timeout: 5_000,
      timeoutMsg: 'Add account modal did not close after picking WhatsApp Web',
    });

    // 2) Redux must record a new account with provider === "whatsapp" — the
    // backing-state mock-effect that proves registration happened, not just
    // that the modal vanished. The Accounts rail tooltip and the modal both
    // render the literal string "WhatsApp Web", so a DOM text assertion
    // alone cannot distinguish them. Store handle from app/src/store/index.ts.
    const registered = await browser.waitUntil(
      async () =>
        await browser.execute(() => {
          const winAny = window as unknown as { __OPENHUMAN_STORE__?: { getState: () => unknown } };
          const state = winAny.__OPENHUMAN_STORE__?.getState() as
            | { accounts?: { accounts?: Record<string, { provider?: string }> } }
            | undefined;
          if (!state) return false;
          const accounts = state.accounts?.accounts ?? {};
          return Object.values(accounts).some(a => a.provider === 'whatsapp');
        }),
      {
        timeout: 5_000,
        timeoutMsg:
          'Redux accounts slice never recorded a whatsapp provider after picking the WhatsApp Web tile',
      }
    );
    expect(registered).toBe(true);
  });
});
