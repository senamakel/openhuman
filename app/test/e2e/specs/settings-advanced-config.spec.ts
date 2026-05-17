// @ts-nocheck
import { browser, expect } from '@wdio/globals';

import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc } from '../helpers/core-rpc';
import { clickText, textExists, waitForText } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-settings-advanced-config';

async function clickSelector(selector: string): Promise<boolean> {
  return await browser.execute(sel => {
    const el = document.querySelector<HTMLElement>(sel);
    if (!el) return false;
    el.click();
    return true;
  }, selector);
}

async function setInput(selector: string, value: string): Promise<boolean> {
  return await browser.execute(
    ({ sel, next }) => {
      const el = document.querySelector<HTMLInputElement>(sel);
      if (!el) return false;
      el.value = next;
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
      return true;
    },
    { sel: selector, next: value }
  );
}

async function readLocalStorageJson<T = unknown>(key: string): Promise<T | null> {
  return await browser.execute(storageKey => {
    const raw = window.localStorage.getItem(storageKey);
    return raw ? JSON.parse(raw) : null;
  }, key);
}

describe('Settings - Advanced Config', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('renders the developer options route and its advanced entries', async () => {
    await navigateViaHash('/settings/developer-options');

    await waitForText('Developer Options', 15_000);
    await waitForText('AI Configuration', 15_000);
    await waitForText('Notification Routing', 15_000);
    await waitForText('Composio Routing (Direct Mode)', 15_000);
    await waitForText('About', 15_000);
  });

  it('persists notification routing settings through core RPC', async () => {
    const before = await callOpenhumanRpc('openhuman.notification_settings_get', {
      provider: 'gmail',
    });
    expect(before.ok).toBe(true);
    const initialEnabled = Boolean(before.result?.settings?.enabled);

    await navigateViaHash('/settings/notification-routing');
    await waitForText('Notification Intelligence', 15_000);
    expect(await clickSelector('input[type="checkbox"]')).toBe(true);

    await browser.waitUntil(async () => {
      const after = await callOpenhumanRpc('openhuman.notification_settings_get', {
        provider: 'gmail',
      });
      return after.ok && Boolean(after.result?.settings?.enabled) === !initialEnabled;
    }, { timeout: 15_000, interval: 500, timeoutMsg: 'notification routing did not persist' });
  });

  it('persists composio trigger triage settings', async () => {
    const before = await callOpenhumanRpc('openhuman.config_get_composio_trigger_settings', {});
    expect(before.ok).toBe(true);

    await navigateViaHash('/settings/composio-triggers');
    await waitForText('Integration Triggers', 15_000);

    expect(await clickSelector('[role="switch"]')).toBe(true);
    expect(await setInput('#disabled-toolkits', 'gmail, slack')).toBe(true);
    await clickText('Save', 10_000);
    await waitForText('Settings saved', 10_000);

    await browser.waitUntil(async () => {
      const after = await callOpenhumanRpc('openhuman.config_get_composio_trigger_settings', {});
      const result = after.result?.result ?? {};
      return (
        after.ok &&
        Array.isArray(result.triage_disabled_toolkits) &&
        result.triage_disabled_toolkits.includes('gmail') &&
        result.triage_disabled_toolkits.includes('slack')
      );
    }, { timeout: 15_000, interval: 500, timeoutMsg: 'composio trigger settings did not persist' });
  });

  it('switches composio routing mode to direct and back to backend', async () => {
    await navigateViaHash('/settings/composio-routing');
    await waitForText('Routing mode', 15_000);

    expect(await clickSelector('input[value="direct"]')).toBe(true);
    expect(await setInput('#composio-api-key', 'ck_live_e2e_composio_key')).toBe(true);
    await clickText('Save', 10_000);
    await waitForText('Switching to Direct mode', 10_000);
    await clickText('I understand, switch to Direct', 10_000);
    await waitForText('Settings saved', 15_000);

    await browser.waitUntil(async () => {
      const mode = await callOpenhumanRpc('openhuman.composio_get_mode', {});
      return (
        mode.ok &&
        mode.result?.result?.mode === 'direct' &&
        mode.result?.result?.api_key_set === true
      );
    }, { timeout: 15_000, interval: 500, timeoutMsg: 'composio direct mode did not persist' });

    expect(await clickSelector('input[value="backend"]')).toBe(true);
    await clickText('Save', 10_000);
    await waitForText('Switched to Backend mode', 15_000);

    const backend = await callOpenhumanRpc('openhuman.composio_get_mode', {});
    expect(backend.ok).toBe(true);
    expect(backend.result?.result?.mode).toBe('backend');
    expect(backend.result?.result?.api_key_set).toBe(false);
  });

  it('persists agent chat draft state to localStorage', async () => {
    await navigateViaHash('/settings/agent-chat');

    await waitForText('Overrides', 15_000);
    expect(await setInput('input[placeholder="gpt-4o"]', 'gpt-4.1-mini')).toBe(true);
    expect(await setInput('input[placeholder="0.7"]', '0.2')).toBe(true);
    expect(await setInput('textarea[placeholder]', 'persist this draft')).toBe(true);

    await browser.waitUntil(async () => {
      const payload = await readLocalStorageJson<{
        modelOverride?: string;
        temperature?: string;
        messages?: Array<{ role: string; text: string }>;
      }>('openhuman.settings.agentChat.history');
      return payload?.modelOverride === 'gpt-4.1-mini' && payload?.temperature === '0.2';
    }, { timeout: 10_000, interval: 250, timeoutMsg: 'agent chat draft did not persist' });
  });

  it('mounts the remaining advanced settings routes', async () => {
    await navigateViaHash('/settings/local-model-debug');
    await waitForText('Local Model Debug', 15_000);

    await navigateViaHash('/settings/about');
    await waitForText('Software Updates', 15_000);

    await navigateViaHash('/settings/llm');
    await waitForText('AI', 20_000);
    expect((await textExists('Cloud providers')) || (await textExists('Primary cloud'))).toBe(true);
  });
});
