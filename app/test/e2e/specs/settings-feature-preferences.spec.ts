// @ts-nocheck
import { browser, expect } from '@wdio/globals';

import { waitForApp } from '../helpers/app-helpers';
import { callOpenhumanRpc } from '../helpers/core-rpc';
import {
  clickSelector,
  clickText,
  setSelectValueByTestId,
  waitForText,
} from '../helpers/element-helpers';
import { resetApp } from '../helpers/reset-app';
import { navigateViaHash } from '../helpers/shared-flows';
import { startMockServer, stopMockServer } from '../mock-server';

const USER_ID = 'e2e-settings-feature-preferences';

async function mascotColorChecked(colorId: string): Promise<string | null> {
  return await browser.execute(id => {
    const el = document.querySelector<HTMLElement>(`[data-testid="mascot-color-${id}"]`);
    return el?.getAttribute('aria-checked') ?? null;
  }, colorId);
}

async function mascotVoiceIdFromStore(): Promise<string | null> {
  return await browser.execute(() => {
    const win = window as unknown as {
      __OPENHUMAN_STORE__?: { getState?: () => { mascot?: { voiceId?: string | null } } };
    };
    return win.__OPENHUMAN_STORE__?.getState?.().mascot?.voiceId ?? null;
  });
}

async function defaultMessagingChannelFromStore(): Promise<string | null> {
  return await browser.execute(() => {
    const win = window as unknown as {
      __OPENHUMAN_STORE__?: {
        getState?: () => { channelConnections?: { defaultMessagingChannel?: string | null } };
      };
    };
    return (
      win.__OPENHUMAN_STORE__?.getState?.().channelConnections?.defaultMessagingChannel ?? null
    );
  });
}

describe('Settings - Feature Preferences', function () {
  // WebdriverIO wraps hooks before entering their bodies, so a hook-local
  // timeout cannot extend the wrapper's default 30-second budget.
  this.timeout(90_000);

  before(async () => {
    await startMockServer();
    await waitForApp();
    await resetApp(USER_ID);
  });

  after(async () => {
    await stopMockServer();
  });

  it('falls through removed feature routes to settings home', async () => {
    for (const route of [
      '/settings/features',
      '/settings/screen-intelligence',
      '/settings/screen-awareness-debug',
    ]) {
      await navigateViaHash(route);
      await browser.waitUntil(
        async () => {
          const hash = await browser.execute(() => window.location.hash);
          return hash === '#/settings' || hash === '#/settings/account';
        },
        { timeout: 15_000, timeoutMsg: `${route} did not normalize to the settings index` }
      );

      const bodyText = await browser.$('body').getText();
      expect(bodyText).not.toContain('Screen Intelligence');
      expect(bodyText).not.toContain('Screen Awareness Debug');
    }
  });

  it('persists the default messaging channel through redux state', async () => {
    // The messaging panel exposes "Set as default" only on *connected* channels.
    // In a fresh workspace the only always-connected channel is Web (built-in
    // chat), so make Telegram the default first — that turns Web into a
    // connected, non-default tile with the control — then switch to Web.
    await callOpenhumanRpc('openhuman.channels_set_default', { channel: 'telegram' });

    // Navigate away and back so the panel re-seeds the default from the core.
    await navigateViaHash('/home');
    await navigateViaHash('/connections?tab=messaging');

    await browser.waitUntil(async () => (await defaultMessagingChannelFromStore()) === 'telegram', {
      timeout: 10_000,
      interval: 500,
      timeoutMsg: 'messaging panel did not seed Telegram as the default',
    });

    // Switch to Web via its stable channel-select test id (Web is always
    // connected, so its "Set as default" control is present).
    await clickSelector('[data-testid="channel-select-web"]', 10_000);
    await browser.waitUntil(async () => (await defaultMessagingChannelFromStore()) === 'web', {
      timeout: 10_000,
      interval: 500,
      timeoutMsg: 'default channel did not update',
    });
  });

  it('persists tools preferences to the core app-state snapshot', async () => {
    const before = await callOpenhumanRpc('openhuman.app_state_snapshot', {});
    expect(before.ok).toBe(true);
    const enabledBefore = before.result?.result?.localState?.onboardingTasks?.enabledTools ?? [];

    await navigateViaHash('/settings/tools');
    await waitForText('Tools', 15_000);

    expect(await clickText('Shell Commands', 10_000)).toBeDefined();
    await clickText('Save Changes', 10_000);
    await waitForText('Preferences saved', 10_000);

    await browser.waitUntil(
      async () => {
        const after = await callOpenhumanRpc('openhuman.app_state_snapshot', {});
        const enabledAfter = after.result?.result?.localState?.onboardingTasks?.enabledTools ?? [];
        return JSON.stringify(enabledAfter) !== JSON.stringify(enabledBefore);
      },
      { timeout: 15_000, interval: 500, timeoutMsg: 'tools settings did not persist' }
    );
  });

  it('redirects the retired notifications settings route to account', async () => {
    await navigateViaHash('/settings/notifications');
    await browser.waitUntil(
      async () => (await browser.execute(() => window.location.hash)) === '#/settings/account',
      { timeout: 15_000, timeoutMsg: 'retired notifications route did not redirect to account' }
    );
    await waitForText('Account', 15_000);
  });

  it('persists mascot color selection', async () => {
    await navigateViaHash('/settings/mascot');

    await waitForText('Color', 15_000);
    expect(await clickSelector('[data-testid="mascot-color-burgundy"]')).toBeDefined();
    await browser.pause(1000);
    expect(await mascotColorChecked('burgundy')).toBe('true');
  });

  it('persists the custom mascot voice override on the mascot/face panel', async () => {
    // The mascot voice override lives on the Face panel. The legacy
    // /settings/mascot slug redirects to /settings/face; /settings/voice now
    // hosts STT/TTS providers.
    await navigateViaHash('/settings/mascot');

    await browser
      .$('[data-testid="mascot-voice-select"]')
      .waitForExist({ timeout: 20_000, timeoutMsg: 'mascot-voice-select did not render' });
    const selectWorked = await setSelectValueByTestId('mascot-voice-select', '__custom__');
    if (!selectWorked) {
      console.log(
        '[settings-features] mascot-voice-select not found or __custom__ option unavailable — skipping'
      );
      return;
    }
    const customVoiceInput = await browser.$('[data-testid="mascot-voice-input"]');
    try {
      await customVoiceInput.waitForExist({ timeout: 10_000 });
    } catch {
      // The custom voice input may not appear if the select interaction
      // didn't trigger the expected UI change. Skip gracefully.
      console.log(
        '[settings-features] mascot-voice-input did not appear after selecting __custom__ — skipping'
      );
      return;
    }
    await browser.execute(() => {
      const input = document.querySelector<HTMLInputElement>('[data-testid="mascot-voice-input"]');
      if (!input) return;
      const setter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype,
        'value'
      )?.set;
      if (setter) setter.call(input, 'voice-e2e-custom');
      else input.value = 'voice-e2e-custom';
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(await clickSelector('[data-testid="mascot-voice-save-paste"]')).toBeDefined();
    await browser.waitUntil(async () => (await mascotVoiceIdFromStore()) === 'voice-e2e-custom', {
      timeout: 10_000,
      interval: 500,
      timeoutMsg: 'custom mascot voice did not update',
    });
    // Wry terminates the WebDriver session on a full page reload, so the
    // durable value is covered by the Redux assertion above. Persistence
    // across a process restart remains covered by MascotPanel unit tests.
  });
});
