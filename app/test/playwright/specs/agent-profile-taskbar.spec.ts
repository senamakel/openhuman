import { expect, type Page, test } from '@playwright/test';

import {
  bootAuthenticatedPage,
  dismissWalkthroughIfPresent,
  waitForAppReady,
} from '../helpers/core-rpc';

async function activeAgentProfileId(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const win = window as unknown as {
      __OPENHUMAN_STORE__?: {
        getState?: () => { agentProfiles?: { activeProfileId?: string | null } };
      };
    };
    return win.__OPENHUMAN_STORE__?.getState?.().agentProfiles?.activeProfileId ?? null;
  });
}

test.describe('Agent profile taskbar switcher', () => {
  test('switches the active agent profile from the bottom taskbar avatar menu', async ({
    page,
  }) => {
    await bootAuthenticatedPage(page, 'pw-agent-profile-taskbar', '/home');
    await waitForAppReady(page);
    await dismissWalkthroughIfPresent(page);

    const switcher = page.getByRole('button', { name: /Switch agent profile:/ });
    await expect(switcher).toBeVisible();
    await expect(switcher).toHaveAttribute('aria-label', 'Switch agent profile: Default');

    await switcher.click();
    await expect(page.getByRole('menu', { name: 'Agent profiles' })).toBeVisible();
    await expect(page.getByRole('menuitemradio', { name: /Default/ })).toHaveAttribute(
      'aria-checked',
      'true'
    );

    await page.getByRole('menuitemradio', { name: /Research/ }).click();

    await expect.poll(() => activeAgentProfileId(page), { timeout: 10_000 }).toBe('research');
    await expect(switcher).toHaveAttribute('aria-label', 'Switch agent profile: Research');
  });
});
