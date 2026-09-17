import { expect, test } from '@playwright/test';

import { bootAuthenticatedPage, waitForAppReady } from '../helpers/core-rpc';

test.describe('Local model runtime flow', () => {
  test('shows direct-runtime guidance instead of app-managed bootstrap controls', async ({
    page,
  }) => {
    await bootAuthenticatedPage(page, 'pw-local-model-runtime');
    // Navigate after the authenticated shell has mounted. Supplying this legacy
    // settings path during bootstrap races its default chat-route restoration.
    await page.goto('/#/settings/local-model-debug');
    await waitForAppReady(page);

    await expect
      .poll(async () => page.evaluate(() => window.location.hash))
      .toContain('/connections');
  });
});
