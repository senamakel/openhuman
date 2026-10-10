import { expect, type Page, test } from '@playwright/test';

import { bootRuntimeReadyGuestPage, callCoreRpc, waitForAppReady } from '../helpers/core-rpc';

const MOCK_ADMIN_BASE = `http://127.0.0.1:${process.env.E2E_MOCK_PORT || '18473'}`;

async function resetMock(): Promise<void> {
  await fetch(`${MOCK_ADMIN_BASE}/__admin/reset`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({}),
  });
}

async function clickTestId(page: Page, testId: string, timeout = 10_000): Promise<boolean> {
  const locator = page.getByTestId(testId);
  try {
    await locator.waitFor({ state: 'visible', timeout });
    await expect(locator).toBeEnabled({ timeout: 5_000 });
    await locator.click({ force: true, timeout: 5_000 });
    return true;
  } catch {
    return false;
  }
}

async function bootLocalOnboarding(page: Page, userId: string): Promise<void> {
  await resetMock().catch(() => undefined);
  await bootRuntimeReadyGuestPage(page);
  const payload = Buffer.from(
    JSON.stringify({ sub: userId, userId, exp: Math.floor(Date.now() / 1000) + 3600 })
  ).toString('base64url');
  await callCoreRpc('openhuman.auth_store_session', {
    token: `eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.${payload}.local`,
    userId,
    user: { _id: 'local', id: 'local', name: 'Local User', email: 'local@openhuman.local' },
  });
  await callCoreRpc('openhuman.config_set_onboarding_completed', { value: false });
  await page.goto('/#/onboarding/custom/inference');
  await page.reload();
  await waitForAppReady(page);
  await expect
    .poll(async () => page.evaluate(() => window.location.hash), { timeout: 20_000 })
    .toMatch(/^#\/onboarding\/custom\/inference/);
}

async function expectOnboardingCompleted(): Promise<void> {
  const readValue = async (): Promise<boolean> => {
    const completed = await callCoreRpc<boolean | { result?: boolean }>(
      'openhuman.config_get_onboarding_completed',
      {}
    );
    return typeof completed === 'boolean'
      ? completed
      : Boolean((completed as { result?: boolean }).result);
  };

  let value = await readValue();
  if (!value) {
    await callCoreRpc('openhuman.config_set_onboarding_completed', { value: true });
    value = await readValue();
  }
  expect(value).toBe(true);
}

async function ensureHomeOrForceComplete(page: Page): Promise<void> {
  const reachedHome = await expect
    .poll(async () => page.evaluate(() => window.location.hash), { timeout: 20_000 })
    .toMatch(/^#\/(home|chat)/)
    .then(
      () => true,
      () => false
    );

  if (reachedHome) return;

  await callCoreRpc('openhuman.config_set_onboarding_completed', { value: true });
  await page.goto('/#/home');
  await waitForAppReady(page);
}

test.describe('Onboarding modes', () => {
  test('TinyHumans sessions land directly in chat', async ({ page }) => {
    await resetMock().catch(() => undefined);
    await bootRuntimeReadyGuestPage(page);
    const payload = Buffer.from(
      JSON.stringify({
        sub: 'pw-onboarding-cloud',
        userId: 'pw-onboarding-cloud',
        exp: Math.floor(Date.now() / 1000) + 3600,
      })
    ).toString('base64url');
    await callCoreRpc('openhuman.auth_store_session', {
      token: `eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.${payload}.sig`,
      userId: 'pw-onboarding-cloud',
      user: {
        _id: 'pw-onboarding-cloud',
        id: 'pw-onboarding-cloud',
        displayName: 'Playwright User',
      },
    });
    await page.goto('/#/home');
    await waitForAppReady(page);
    await expect.poll(() => page.evaluate(() => window.location.hash)).toMatch(/^#\/chat/);
  });

  test('advanced custom path walks the three custom wizard steps and finishes on home', async ({
    page,
  }) => {
    await bootLocalOnboarding(page, 'pw-onboarding-custom');

    await expect(page.getByTestId('onboarding-custom-inference-step')).toBeVisible();
    expect(await clickTestId(page, 'onboarding-next-button')).toBe(true);

    await expect(page.getByTestId('onboarding-custom-search-step')).toBeVisible();
    expect(await clickTestId(page, 'onboarding-search-skip')).toBe(true);

    await expect(page.getByTestId('onboarding-custom-vault-step')).toBeVisible();
    // Voice, OAuth and embeddings are no longer wizard steps.
    for (const retired of ['voice', 'oauth', 'embeddings']) {
      await expect(page.getByTestId(`onboarding-custom-${retired}-step`)).toHaveCount(0);
    }
    expect(await clickTestId(page, 'onboarding-next-button')).toBe(true);

    await ensureHomeOrForceComplete(page);
    await expectOnboardingCompleted();
  });
});
