// @ts-nocheck
/**
 * Smoke spec for the Slack account integration (feature 10.1.4).
 *
 * Proves the Accounts page exposes Slack as an addable provider, the Add
 * Account modal lists it with its label + description, and selecting it
 * dismisses the picker and registers an account on the rail.
 *
 * Deferred to follow-up PRs:
 *   - Real Slack OAuth happy path (workspace selection, scope grant)
 *   - Inbound channel sync (10.3.x)
 *   - Send / reply / thread (10.4.x)
 */
import { waitForApp } from '../helpers/app-helpers';
import { clickButton, textExists, waitForText } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash, openAddAccountModal } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-slack-flow';

describe('Slack account integration smoke', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('shows Slack as an addable provider in the Add Account modal', async () => {
    await navigateViaHash('/accounts');
    await waitForText('Add app', 15_000);

    await openAddAccountModal();

    await waitForText('Slack', 10_000);
    expect(await textExists('Slack')).toBe(true);
    expect(await textExists('Slack workspaces and channels.')).toBe(true);
  });

  it('selecting Slack closes the modal and registers an account on the rail', async () => {
    // Re-open the picker so this case is runnable in isolation.
    await navigateViaHash('/accounts');
    await waitForText('Add app', 15_000);
    await openAddAccountModal();
    await waitForText('Slack', 10_000);

    await clickButton('Slack');

    // 1) Modal must close.
    await browser.waitUntil(async () => !(await textExists('Add account')), {
      timeout: 5_000,
      timeoutMsg: 'Add account modal did not close after picking Slack',
    });

    // 2) Redux must record a new account with provider === "slack". The
    // store handle is exposed on `window.__OPENHUMAN_STORE__` (see
    // app/src/store/index.ts); a pure DOM assertion can't distinguish the
    // Slack tile label from the post-pick rail tooltip — both render the
    // literal "Slack" — so we read state directly.
    const registered = await browser.waitUntil(
      async () =>
        await browser.execute(() => {
          const winAny = window as unknown as { __OPENHUMAN_STORE__?: { getState: () => unknown } };
          const state = winAny.__OPENHUMAN_STORE__?.getState() as
            | { accounts?: { accounts?: Record<string, { provider?: string }> } }
            | undefined;
          if (!state) return false;
          const accounts = state.accounts?.accounts ?? {};
          return Object.values(accounts).some(a => a.provider === 'slack');
        }),
      {
        timeout: 5_000,
        timeoutMsg:
          'Redux accounts slice never recorded a slack provider after picking the Slack tile',
      }
    );
    expect(registered).toBe(true);
  });
});
