// @ts-nocheck
/**
 * Settings - Search: the multi-provider web search panel.
 *
 * Drives the panel through its stable test ids and checks what the core
 * persisted through `openhuman.config_get_search_settings`: the global switch
 * (`enabled`), a provider's enable flag (`providers[]`), and which providers
 * serve each role (`effective_roles`).
 */
import { browser, expect } from '@wdio/globals';

import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc } from '../helpers/core-rpc';
import { clickTestId, waitForTestId } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-settings-search';

type SearchSettings = {
  enabled?: boolean;
  providers?: Array<{ id: string; enabled: boolean; route: string }>;
  effective_roles?: Record<string, string[]>;
};

async function getSearchSettings(): Promise<SearchSettings> {
  const response = await callOpenhumanRpc('openhuman.config_get_search_settings', {});
  expect(response.ok).toBe(true);
  return (response.result?.result ?? {}) as SearchSettings;
}

function providerOf(settings: Record<string, unknown>, id: string) {
  return (settings.providers ?? []).find(p => p.id === id);
}

async function waitForSettings(
  predicate: (settings: Record<string, unknown>) => boolean,
  timeoutMsg: string
): Promise<void> {
  await browser.waitUntil(async () => predicate(await getSearchSettings()), {
    timeout: 10_000,
    interval: 500,
    timeoutMsg,
  });
}

describe('Settings - Search', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('turns search off and on from the global switch', async () => {
    const reset = await callOpenhumanRpc('openhuman.config_update_search_settings', {
      enabled: true,
      providers: { brave: { enabled: false } },
      roles: { search: [] },
    });
    expect(reset.ok).toBe(true);

    await navigateViaHash('/settings/search');
    await waitForTestId('search-settings-panel', 15_000);
    await waitForTestId('search-provider-exa', 15_000);
    await waitForTestId('search-role-search', 15_000);

    await clickTestId('search-enabled-toggle');
    await waitForSettings(
      settings =>
        settings.enabled === false &&
        Object.values(settings.effective_roles ?? {}).every(order => order.length === 0),
      'search was not turned off in core config'
    );

    await clickTestId('search-enabled-toggle');
    await waitForSettings(
      settings => settings.enabled === true,
      'search was not turned back on in core config'
    );

    const settings = await getSearchSettings();
    // Every provider the core serves a role with must be one it reports as usable.
    for (const order of Object.values(settings.effective_roles ?? {})) {
      for (const id of order) {
        expect(providerOf(settings, id)?.usable).toBe(true);
      }
    }
  });

  it('enables a bring-your-own-key provider without making it serve a role', async () => {
    await waitForTestId('search-provider-brave-toggle', 15_000);
    await clickTestId('search-provider-brave-toggle');

    await waitForSettings(
      settings => providerOf(settings, 'brave')?.enabled === true,
      'brave was not enabled in core config'
    );

    const settings = await getSearchSettings();
    const brave = providerOf(settings, 'brave');
    // No key stored in a fresh profile, so Brave needs one and serves nothing.
    expect(brave.status).toBe('needs_key');
    expect(settings.effective_roles.search).not.toContain('brave');
    await waitForTestId('search-role-search-provider-brave', 10_000);
  });
});
