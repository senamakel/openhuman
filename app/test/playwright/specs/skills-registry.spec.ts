import { expect, test } from '@playwright/test';

import {
  bootRuntimeReadyGuestPage,
  callCoreRpc,
  dismissWalkthroughIfPresent,
  signInViaCallbackToken,
  waitForAppReady,
} from '../helpers/core-rpc';

async function openSkillsPage(page: Parameters<typeof test>[0]['page'], userId: string) {
  await bootRuntimeReadyGuestPage(page);
  await signInViaCallbackToken(page, userId);
  await page.evaluate(() => {
    try {
      localStorage.setItem('openhuman:walkthrough_completed', 'true');
      localStorage.removeItem('openhuman:walkthrough_pending');
    } catch {}
    window.location.hash = '/skills';
  });
  await expect
    .poll(async () => page.evaluate(() => window.location.hash), { timeout: 10_000 })
    .toContain('/skills');
  await waitForAppReady(page);
  await dismissWalkthroughIfPresent(page);
}

test.describe('Skills registry flow', () => {
  test.beforeEach(async ({ page }, testInfo) => {
    const testSlug = testInfo.title.toLowerCase().replace(/[^a-z0-9]+/g, '-');
    await openSkillsPage(page, 'pw-skills-registry-' + testSlug);
  });

  test('navigates to /skills and renders the current tabs', async ({ page }) => {
    await expect(page.getByRole('tab', { name: 'Composio' })).toBeVisible();
    await expect(page.getByRole('tab', { name: 'Channels' })).toBeVisible();
    await expect(page.getByRole('tab', { name: 'MCP Servers' })).toBeVisible();
    await page.getByRole('tab', { name: 'Composio' }).click();
    await expect(page.getByRole('heading', { name: 'Composio Integrations' })).toBeVisible();
    await expect(
      page.getByText(/Gmail|Notion|Telegram|GitHub|Google Drive/, { exact: false }).first()
    ).toBeVisible();
  });

  test('shows at least one known Composio integration name', async ({ page }) => {
    await expect(
      page.getByText(/Gmail|Notion|Telegram|GitHub|Google Drive/, { exact: false }).first()
    ).toBeVisible();
  });

  test('channels tab renders messaging connectors', async ({ page }) => {
    await page.getByRole('tab', { name: 'Channels' }).click();
    await expect(page.getByRole('heading', { name: 'Channels' })).toBeVisible();
    await expect(page.getByText(/Telegram|Discord|Slack/).first()).toBeVisible();
  });

  test('mcp tab shows the placeholder panel', async ({ page }) => {
    await page.getByRole('tab', { name: 'MCP Servers' }).click();
    await expect(page.getByRole('heading', { name: 'MCP Servers' }).first()).toBeVisible();
    await expect(page.getByText(/coming soon|early alpha|MCP/i).first()).toBeVisible();
  });
});

test.describe('Skill registry RPC smoke', () => {
  // These tests call the core RPC directly — no page navigation needed.
  // They verify the skill_registry module responds correctly.

  test('sources returns the three default registries', async () => {
    const result = await callCoreRpc<{
      sources: Array<{ id: string; name: string; kind: string; enabled: boolean }>;
    }>('openhuman.skill_registry_sources');
    expect(result.sources).toBeDefined();
    expect(result.sources.length).toBeGreaterThanOrEqual(3);

    const ids = result.sources.map(s => s.id);
    expect(ids).toContain('openhuman-community');
    expect(ids).toContain('hermeshub');
    expect(ids).toContain('clawhub');

    for (const source of result.sources) {
      expect(source.name).toBeTruthy();
      expect(source.kind).toBeTruthy();
      expect(typeof source.enabled).toBe('boolean');
    }
  });

  test('browse returns catalog entries from at least one source', async () => {
    test.setTimeout(30_000);
    const result = await callCoreRpc<{
      entries: Array<{
        id: string;
        name: string;
        source_id: string;
        format: string;
        download_url: string;
      }>;
    }>('openhuman.skill_registry_browse', { force_refresh: true });
    expect(result.entries).toBeDefined();
    expect(result.entries.length).toBeGreaterThan(0);

    // At least the openhuman-community source should have our git-summary skill
    const openhumanEntries = result.entries.filter(e => e.source_id === 'openhuman-community');
    expect(openhumanEntries.length).toBeGreaterThan(0);

    // Each entry has required fields
    for (const entry of result.entries.slice(0, 5)) {
      expect(entry.id).toBeTruthy();
      expect(entry.name).toBeTruthy();
      expect(entry.source_id).toBeTruthy();
      expect(entry.format).toBeTruthy();
    }
  });

  test('search filters entries by query', async () => {
    test.setTimeout(30_000);
    const result = await callCoreRpc<{
      entries: Array<{ id: string; name: string; description: string }>;
    }>('openhuman.skill_registry_search', { query: 'git' });
    expect(result.entries).toBeDefined();
    // Should find at least git-summary
    const gitSummary = result.entries.find(e => e.id === 'git-summary');
    expect(gitSummary).toBeDefined();
  });

  test('search with empty query returns all entries', async () => {
    test.setTimeout(30_000);
    const all = await callCoreRpc<{ entries: Array<unknown> }>(
      'openhuman.skill_registry_search',
      { query: '' }
    );
    expect(all.entries.length).toBeGreaterThan(0);
  });
});
