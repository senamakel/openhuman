// @ts-nocheck
/**
 * Chat background-activity panel — end-to-end.
 *
 * The former chat-header background tasks drawer was removed from the current
 * conversation surface. Keep a desktop E2E guard against restoring its retired
 * toggle while opening a fresh thread.
 */
import { waitForApp } from '../helpers/app-helpers';
import { chatMounted, clickByTitle, getSelectedThreadId } from '../helpers/chat-harness';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const LOG_PREFIX = '[chat-background-activity-panel]';
const USER_ID = 'e2e-chat-background-activity-panel';

describe('Chat background-activity panel', () => {
  before(async function beforeSuite() {
    this.timeout(90_000);
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
    console.log(`${LOG_PREFIX} setup complete`);
  });

  after(async () => {
    await stopMockServer();
  });

  it('does not render the retired Background tasks toggle in chat', async () => {
    await navigateViaHash('/chat');
    await browser.waitUntil(async () => await chatMounted(), {
      timeout: 15_000,
      timeoutMsg: 'Conversations panel did not mount',
    });

    // The header toggle only renders once a thread is selected.
    expect(await clickByTitle('New thread', 8_000)).toBe(true);
    await browser.waitUntil(async () => await getSelectedThreadId(), {
      timeout: 8_000,
      timeoutMsg: 'thread.selectedThreadId never populated',
    });

    const toggle = await $('[data-testid="background-processes-toggle"]');
    expect(await toggle.isExisting()).toBe(false);
    const panel = await $('[data-testid="background-processes-panel"]');
    expect(await panel.isExisting()).toBe(false);
    console.log(`${LOG_PREFIX} retired background tasks UI remains absent`);
  });
});
