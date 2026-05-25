import { expect, test, type Page } from '@playwright/test';

import {
  bootRuntimeReadyGuestPage,
  callCoreRpc,
  dismissWalkthroughIfPresent,
  signInViaBypassUser,
  waitForAppReady,
} from '../helpers/core-rpc';

const GUARD_TOOLKITS = ['github', 'gmail', 'slack', 'notion', 'discord'] as const;
const MOCK_BASE = 'http://127.0.0.1:' + (process.env.E2E_MOCK_PORT || '18473');

async function mockFetch(path: string, init?: RequestInit) {
  const response = await fetch(MOCK_BASE + path, init);
  if (!response.ok) throw new Error('mock request failed: ' + response.status + ' ' + path);
  return response.json() as Promise<{ data?: unknown }>;
}

async function resetMock() {
  await mockFetch('/__admin/reset', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ keepBehavior: false, keepRequests: false }),
  });
}

async function setMockBehavior(behavior: Record<string, unknown>) {
  await mockFetch('/__admin/behavior', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ behavior }),
  });
}

async function seedGuardConnections(status: 'ACTIVE' | 'FAILED' | 'EXPIRED' = 'ACTIVE') {
  await setMockBehavior({
    composioToolkits: JSON.stringify(GUARD_TOOLKITS),
    composioConnections: JSON.stringify(
      GUARD_TOOLKITS.map((slug, index) => ({ id: `c-guard-${index}`, toolkit: slug, status }))
    ),
  });
}

async function bootSkillsPage(page: Page, userId: string) {
  await resetMock();
  await seedGuardConnections();
  await bootRuntimeReadyGuestPage(page);
  try {
    await signInViaBypassUser(page, userId);
  } catch {
    await bootRuntimeReadyGuestPage(page);
    await signInViaBypassUser(page, userId);
  }
  await page.goto('/#/skills');
  await waitForAppReady(page);
  await dismissWalkthroughIfPresent(page);
  const tile = page.getByTestId('skill-install-composio-github');
  const connectionsButton = page.getByRole('button', { name: 'Connections' });
  if (await connectionsButton.isVisible().catch(() => false)) {
    await connectionsButton.click();
    await waitForAppReady(page);
    await dismissWalkthroughIfPresent(page);
  }
  await expect(tile).toBeVisible({ timeout: 20_000 });
}

async function reloadSkills(page: Page) {
  await page.goto('/#/skills');
  await waitForAppReady(page);
  await dismissWalkthroughIfPresent(page);
}

async function assertSessionNotNuked(page: Page) {
  await expect
    .poll(async () =>
      page.evaluate(() => {
        const win = window as typeof window & {
          __OPENHUMAN_CORE_STATE__?: () => {
            snapshot?: {
              sessionToken?: string | null;
              currentUser?: { _id?: string | null } | null;
            };
          };
        };
        const snapshot = win.__OPENHUMAN_CORE_STATE__?.()?.snapshot;
        return {
          hash: window.location.hash,
          hasToken: Boolean(snapshot?.sessionToken),
          hasUser: Boolean(snapshot?.currentUser?._id),
        };
      })
    )
    .toEqual({ hash: '#/skills', hasToken: true, hasUser: true });
}

test.describe('Connector session guard', () => {
  test.beforeEach(async ({ page }, testInfo) => {
    const testSlug = testInfo.title.toLowerCase().replace(/[^a-z0-9]+/g, '-');
    await bootSkillsPage(page, 'pw-session-guard-' + testSlug);
  });

  test('survives execute failures across toolkits', async ({ page }) => {
    await setMockBehavior({ composioExecuteFails: '400' });
    for (const [index, toolkit] of GUARD_TOOLKITS.entries()) {
      await expect(
        callCoreRpc('openhuman.composio_execute', {
          tool: `${toolkit.toUpperCase()}_TEST_ACTION`,
          arguments: {},
        })
      ).rejects.toThrow(/failed/i);
    }
    await assertSessionNotNuked(page);
  });

  test('survives execute 500-class failures across toolkits', async ({ page }) => {
    await setMockBehavior({ composioExecuteFails: '500' });
    for (const [index, toolkit] of GUARD_TOOLKITS.entries()) {
      await expect(
        callCoreRpc('openhuman.composio_execute', {
          tool: `${toolkit.toUpperCase()}_TEST_ACTION`,
          arguments: {},
        })
      ).rejects.toThrow(/failed/i);
    }
    await assertSessionNotNuked(page);
  });

  test('survives delete failures across toolkits', async ({ page }) => {
    await setMockBehavior({ composioDeleteFails: '1' });
    for (const [index] of GUARD_TOOLKITS.entries()) {
      await expect(
        callCoreRpc('openhuman.composio_delete_connection', { connection_id: `c-guard-${index}` })
      ).rejects.toThrow(/failed/i);
    }
    await assertSessionNotNuked(page);
  });

  test.skip('survives sync failures across toolkits', async ({ page }) => {
    await setMockBehavior({ composioSyncFails: '1' });
    for (const [index, toolkit] of GUARD_TOOLKITS.entries()) {
      await expect(
        callCoreRpc('openhuman.composio_sync', {
          connection_id: `c-guard-${index}`,
        })
      ).rejects.toThrow(/failed/i);
    }
    await assertSessionNotNuked(page);
  });

  test('survives rendering FAILED connections on the skills page', async ({ page }) => {
    await seedGuardConnections('FAILED');
    await reloadSkills(page);
    await assertSessionNotNuked(page);
  });

  test('survives rendering EXPIRED connections on the skills page', async ({ page }) => {
    await seedGuardConnections('EXPIRED');
    await reloadSkills(page);
    await assertSessionNotNuked(page);
  });

  test('survives rapid authorize plus execute failures across toolkits', async ({ page }) => {
    await setMockBehavior({ composioExecuteFails: '1', composioDeleteFails: '1' });
    for (const [index, toolkit] of GUARD_TOOLKITS.entries()) {
      await callCoreRpc('openhuman.composio_authorize', { toolkit });
      await expect(
        callCoreRpc('openhuman.composio_execute', {
          tool: `${toolkit.toUpperCase()}_TEST_ACTION`,
          arguments: {},
        })
      ).rejects.toThrow(/failed/i);
    }
    await assertSessionNotNuked(page);
  });
});
