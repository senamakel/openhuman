import { expect, test } from '@playwright/test';

import {
  bootAuthenticatedPage,
  dismissWalkthroughIfPresent,
  waitForAppReady,
} from '../helpers/core-rpc';

test.describe('Google Meet Connections tab', () => {
  test.beforeEach(async ({ page }) => {
    await bootAuthenticatedPage(page, 'pw-gmeet-connections-tab-user', '/skills?tab=meetings');
    await waitForAppReady(page);
    await dismissWalkthroughIfPresent(page);
  });

  test('opens the dedicated tab and shows a one-field meeting link modal', async ({ page }) => {
    await expect
      .poll(async () => page.evaluate(() => window.location.hash), { timeout: 10_000 })
      .toContain('/skills?tab=meetings');

    await expect(page.getByRole('tab', { name: 'Google Meet', exact: true })).toHaveAttribute(
      'aria-selected',
      'true'
    );

    await page.getByTestId('meeting-bots-banner').click();

    const dialog = page.getByRole('dialog', { name: 'Send OpenHuman to a meeting' });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByLabel('Meeting link')).toBeVisible();
    await expect(dialog.locator('input[type="url"]')).toHaveCount(1);
    await expect(dialog.locator('input[type="text"]')).toHaveCount(0);
    await expect(dialog.getByText('Wake Phrase')).toHaveCount(0);
    await expect(dialog.getByText('Display name')).toHaveCount(0);
    await expect(dialog.getByText('Zoom')).toHaveCount(0);
    await expect(dialog.getByText('Microsoft Teams')).toHaveCount(0);
  });
});
