// @ts-nocheck
import { browser, expect } from '@wdio/globals';

import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc } from '../helpers/core-rpc';
import { clickText, textExists, waitForText } from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-settings-feature-preferences';

async function readPersistedSlice(key: string): Promise<Record<string, unknown> | null> {
  return await browser.execute(sliceKey => {
    const raw = window.localStorage.getItem(`persist:${sliceKey}`);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Record<string, string>;
    const hydrated: Record<string, unknown> = {};
    for (const [field, value] of Object.entries(parsed)) {
      try {
        hydrated[field] = JSON.parse(value);
      } catch {
        hydrated[field] = value;
      }
    }
    return hydrated;
  }, key);
}

async function setSelectValue(testId: string, value: string): Promise<boolean> {
  return await browser.execute(
    ({ id, next }) => {
      const el = document.querySelector<HTMLSelectElement>(`[data-testid="${id}"]`);
      if (!el) return false;
      el.value = next;
      el.dispatchEvent(new Event('change', { bubbles: true }));
      return true;
    },
    { id: testId, next: value }
  );
}

async function setInputValue(testId: string, value: string): Promise<boolean> {
  return await browser.execute(
    ({ id, next }) => {
      const el = document.querySelector<HTMLInputElement>(`[data-testid="${id}"]`);
      if (!el) return false;
      el.value = next;
      el.dispatchEvent(new Event('input', { bubbles: true }));
      return true;
    },
    { id: testId, next: value }
  );
}

async function clickSelector(selector: string): Promise<boolean> {
  return await browser.execute(sel => {
    const el = document.querySelector<HTMLElement>(sel);
    if (!el) return false;
    el.click();
    return true;
  }, selector);
}

describe('Settings - Feature Preferences', () => {
  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('renders the features settings section route', async () => {
    await navigateViaHash('/settings/features');

    await waitForText('Features', 15_000);
    await waitForText('Screen Awareness', 15_000);
    await waitForText('Messaging Channels', 15_000);
    await waitForText('Notifications', 15_000);
    await waitForText('Tools', 15_000);
  });

  it('persists the default messaging channel through redux state', async () => {
    await navigateViaHash('/settings/messaging');

    await waitForText('Default Messaging Channel', 15_000);
    await clickText('Discord', 10_000);
    await waitForText('Active route: discord via', 5_000);

    await browser.waitUntil(async () => {
      const persisted = await readPersistedSlice('channelConnections');
      return persisted?.defaultMessagingChannel === 'discord';
    }, { timeout: 10_000, interval: 250, timeoutMsg: 'default channel did not persist' });
  });

  it('persists tools preferences to the core app-state snapshot', async () => {
    const before = await callOpenhumanRpc('openhuman.app_state_snapshot', {});
    expect(before.ok).toBe(true);
    const enabledBefore = before.result?.result?.localState?.onboardingTasks?.enabledTools ?? [];

    await navigateViaHash('/settings/tools');
    await waitForText('Tools', 15_000);

    expect(await clickText('Filesystem', 10_000)).toBeDefined();
    await clickText('Save Changes', 10_000);
    await waitForText('Preferences saved', 10_000);

    await browser.waitUntil(async () => {
      const after = await callOpenhumanRpc('openhuman.app_state_snapshot', {});
      const enabledAfter = after.result?.result?.localState?.onboardingTasks?.enabledTools ?? [];
      return JSON.stringify(enabledAfter) !== JSON.stringify(enabledBefore);
    }, { timeout: 15_000, interval: 500, timeoutMsg: 'tools settings did not persist' });
  });

  it('persists notifications DND and category preferences', async () => {
    const dndBefore = await browser.execute(async () => {
      const invoker = (window as unknown as { __TAURI_INTERNALS__?: { invoke?: Function } })
        .__TAURI_INTERNALS__?.invoke;
      if (typeof invoker !== 'function') return null;
      return await invoker('webview_notification_get_bypass_prefs');
    });

    await navigateViaHash('/settings/notifications');

    await waitForText('Do Not Disturb', 15_000);
    await waitForText('Messages', 15_000);

    expect(await clickSelector('button[aria-label="Toggle Do Not Disturb"]')).toBe(true);
    expect(await clickSelector('button[aria-label="Toggle Messages notifications"]')).toBe(true);

    await browser.waitUntil(async () => {
      const prefs = await browser.execute(async () => {
        const invoker = (window as unknown as { __TAURI_INTERNALS__?: { invoke?: Function } })
          .__TAURI_INTERNALS__?.invoke;
        if (typeof invoker !== 'function') return null;
        return await invoker('webview_notification_get_bypass_prefs');
      });
      const persisted = await readPersistedSlice('notifications');
      return (
        prefs != null &&
        typeof prefs === 'object' &&
        prefs.global_dnd === !Boolean(dndBefore?.global_dnd) &&
        persisted?.preferences != null &&
        (persisted.preferences as Record<string, boolean>).messages === false
      );
    }, { timeout: 15_000, interval: 500, timeoutMsg: 'notification preferences did not persist' });
  });

  it('persists mascot color selection', async () => {
    await navigateViaHash('/settings/mascot');

    await waitForText('Color', 15_000);
    expect(await clickSelector('[data-testid="mascot-color-burgundy"]')).toBe(true);

    await browser.waitUntil(async () => {
      const persisted = await readPersistedSlice('mascot');
      return persisted?.color === 'burgundy';
    }, { timeout: 10_000, interval: 250, timeoutMsg: 'mascot color did not persist' });
  });

  it('persists the custom mascot voice override on the voice panel', async () => {
    await navigateViaHash('/settings/voice');

    await waitForText('Mascot Voice', 20_000);
    expect(await setSelectValue('mascot-voice-select', '__custom__')).toBe(true);
    expect(await setInputValue('mascot-voice-input', 'voice-e2e-custom')).toBe(true);
    expect(await clickSelector('[data-testid="mascot-voice-save-paste"]')).toBe(true);

    await browser.waitUntil(async () => {
      const persisted = await readPersistedSlice('mascot');
      return persisted?.voiceId === 'voice-e2e-custom';
    }, { timeout: 10_000, interval: 250, timeoutMsg: 'mascot voice override did not persist' });
    expect(await textExists('current:')).toBe(true);
  });
});
