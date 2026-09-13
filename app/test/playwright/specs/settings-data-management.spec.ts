import { expect, test } from '@playwright/test';

import {
  bootAuthenticatedPage,
  dismissWalkthroughIfPresent,
  waitForAppReady,
} from '../helpers/core-rpc';

test.describe('Settings - Data Management', () => {
  test.beforeEach(async ({ page }) => {
    await bootAuthenticatedPage(page, 'pw-settings-data-user');
  });

  test('shows Clear App Data confirmation dialog and handles cancel', async ({ page }) => {
    await page.goto('/#/settings/account');
    await waitForAppReady(page);
    await dismissWalkthroughIfPresent(page);

    const clearData = page.getByTestId('settings-nav-logout-and-clear');
    await expect(clearData).toBeVisible();
    await clearData.click();
    await expect(
      page.getByText('This will sign you out and permanently delete local app data')
    ).toBeVisible();

    await page.getByRole('button', { name: 'Cancel' }).click();
    await expect(
      page.getByText('This will sign you out and permanently delete local app data')
    ).toHaveCount(0);
    await expect(clearData).toBeVisible();
  });
});
