import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { beforeEach, describe, expect, type Mock, test, vi } from 'vitest';

import { startLoopbackOauthListener } from '../loopbackOauthListener';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn(() => true) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

type TauriInternalsHolder = { __TAURI_INTERNALS__?: { invoke: unknown } };

const mockInvoke = invoke as Mock;
const mockListen = listen as Mock;

beforeEach(() => {
  vi.clearAllMocks();
  // Satisfy the isTauri() bootstrap-gap check in utils/tauriCommands/common.ts.
  const holder = window as unknown as TauriInternalsHolder;
  holder.__TAURI_INTERNALS__ = { invoke: () => undefined };
});

describe('startLoopbackOauthListener', () => {
  test('returns null when shell bind fails (fallback to deep link)', async () => {
    mockInvoke.mockRejectedValueOnce(new Error('bind 127.0.0.1:53824 failed: Address in use'));

    const handle = await startLoopbackOauthListener();

    expect(handle).toBeNull();
    expect(mockInvoke).toHaveBeenCalledWith('start_loopback_oauth_listener', {
      port: 53824,
      timeoutSecs: 300,
    });
  });

  test('returns handle with redirect uri and state on success', async () => {
    mockInvoke.mockResolvedValueOnce({
      redirectUri: 'http://127.0.0.1:53824/auth',
      state: 'deadbeef',
    });
    mockListen.mockResolvedValue(() => {});

    const handle = await startLoopbackOauthListener();

    expect(handle).not.toBeNull();
    expect(handle!.state).toBe('deadbeef');
    expect(handle!.redirectUri).toBe('http://127.0.0.1:53824/auth?state=deadbeef');
  });

  test('awaitCallback resolves with URL when shell emits callback event', async () => {
    mockInvoke.mockResolvedValueOnce({
      redirectUri: 'http://127.0.0.1:53824/auth',
      state: 'state-1',
    });
    let registered: ((event: { payload: { url: string } }) => void) | null = null;
    mockListen.mockImplementation((_event, handler) => {
      registered = handler;
      return Promise.resolve(() => {});
    });

    const handle = await startLoopbackOauthListener();
    const callbackPromise = handle!.awaitCallback();
    // Wait a microtask for listen() to register.
    await Promise.resolve();
    registered!({ payload: { url: 'http://127.0.0.1:53824/auth?token=jwt&state=state-1' } });

    await expect(callbackPromise).resolves.toBe(
      'http://127.0.0.1:53824/auth?token=jwt&state=state-1'
    );
  });

  test('cancel calls stop_loopback_oauth_listener', async () => {
    mockInvoke
      .mockResolvedValueOnce({ redirectUri: 'http://127.0.0.1:53824/auth', state: 's' })
      .mockResolvedValueOnce(undefined);
    mockListen.mockResolvedValue(() => {});

    const handle = await startLoopbackOauthListener();
    await handle!.cancel();

    expect(mockInvoke).toHaveBeenNthCalledWith(2, 'stop_loopback_oauth_listener');
  });
});
