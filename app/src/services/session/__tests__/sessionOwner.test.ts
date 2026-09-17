import { invoke } from '@tauri-apps/api/core';
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

import { callCoreRpc } from '../../coreRpcClient';

vi.mock('../../coreRpcClient', () => ({ callCoreRpc: vi.fn() }));
vi.mock('../../backendUrl', () => ({
  getBackendUrl: vi.fn().mockResolvedValue('https://api.test'),
}));
vi.mock('../../clientVersionHeaders', () => ({
  getClientVersionHeaders: vi.fn().mockResolvedValue({ 'x-tauri-version': '1.0.0' }),
}));

const isTauriMock = vi.hoisted(() => vi.fn(() => false));
vi.mock('../../../utils/tauriCommands/common', async importOriginal => {
  const actual = await importOriginal<typeof import('../../../utils/tauriCommands/common')>();
  return {
    ...actual,
    isTauri: isTauriMock,
    safeInvoke: (...args: unknown[]) =>
      (invoke as unknown as (...a: unknown[]) => unknown)(...args),
  };
});
const getStoredCoreMode = vi.hoisted(() =>
  vi.fn<() => 'local' | 'cloud' | 'gateway' | null>(() => null)
);
vi.mock('../../../utils/configPersistence', () => ({ getStoredCoreMode }));

// The global test setup mocks this module; these tests exercise the real one.
vi.unmock('../sessionOwner');
const owner = await vi.importActual<typeof import('../sessionOwner')>('../sessionOwner');

const mockInvoke = invoke as Mock;
const mockCallCoreRpc = callCoreRpc as Mock;

const jsonResponse = (status: number, body: unknown): Response =>
  ({ ok: status >= 200 && status < 300, status, json: async () => body }) as unknown as Response;

describe('sessionOwner (desktop shell owner)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    isTauriMock.mockReturnValue(true);
    getStoredCoreMode.mockReturnValue('local');
  });

  it('routes login, store, logout and current user to the shell commands', async () => {
    mockInvoke.mockResolvedValue({ user: { id: 'u1' }, stale: false, staleSeconds: 0 });

    await owner.loginWithToken('login-tok');
    expect(mockInvoke).toHaveBeenCalledWith('auth_login_with_token', { token: 'login-tok' });

    await owner.storeSessionToken('jwt', { id: 'u1' });
    expect(mockInvoke).toHaveBeenCalledWith('auth_store_session', {
      token: 'jwt',
      user: { id: 'u1' },
    });

    await owner.logoutSession();
    expect(mockInvoke).toHaveBeenCalledWith('auth_logout');

    const current = await owner.fetchCurrentUser(true);
    expect(mockInvoke).toHaveBeenCalledWith('auth_current_user', { force: true });
    expect(current).toEqual({ user: { id: 'u1' }, stale: false, staleSeconds: 0 });
    expect(mockCallCoreRpc).not.toHaveBeenCalled();
  });

  it('is not the owner in cloud mode even inside Tauri', () => {
    getStoredCoreMode.mockReturnValue('cloud');
    expect(owner.useShellSessionOwner()).toBe(false);
  });
});

describe('sessionOwner (browser / cloud owner)', () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    isTauriMock.mockReturnValue(false);
    getStoredCoreMode.mockReturnValue(null);
    owner.resetBrowserCurrentUserCache();
    vi.stubGlobal('fetch', fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('exchanges a login token, validates it, and hands the credential to the core', async () => {
    fetchMock
      .mockResolvedValueOnce(jsonResponse(200, { success: true, data: { jwt: 'jwt-1' } }))
      .mockResolvedValueOnce(
        jsonResponse(200, { success: true, data: { _id: 'u9', email: 'a@b' } })
      );
    mockCallCoreRpc.mockResolvedValue({ result: {} });

    await owner.loginWithToken('login-tok');

    expect(fetchMock.mock.calls[0][0]).toBe('https://api.test/auth/login-token/consume');
    expect(JSON.parse(fetchMock.mock.calls[0][1].body)).toEqual({ token: 'login-tok' });
    expect(fetchMock.mock.calls[1][0]).toBe('https://api.test/auth/me');
    expect(fetchMock.mock.calls[1][1].headers.Authorization).toBe('Bearer jwt-1');
    for (const call of fetchMock.mock.calls) {
      expect(call[1].headers['x-sdk-name']).toBe('openhuman');
      expect(call[1].headers['x-tauri-version']).toBe('1.0.0');
    }
    expect(mockCallCoreRpc).toHaveBeenCalledWith({
      method: 'openhuman.auth_set_credential',
      params: { token: 'jwt-1', kind: 'session', userId: 'u9', user: { _id: 'u9', email: 'a@b' } },
    });
  });

  it('never installs a JWT the backend rejects', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse(401, { success: false, message: 'nope' }));

    await expect(owner.storeSessionToken('bad-jwt')).rejects.toThrow(/^REJECTED:/);
    expect(mockCallCoreRpc).not.toHaveBeenCalled();
  });

  it('reports an unreachable backend as transient', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse(503, {}));

    await expect(owner.storeSessionToken('jwt')).rejects.toThrow(/^TRANSIENT:/);
    expect(mockCallCoreRpc).not.toHaveBeenCalled();
  });

  it('stores a local token as-is without touching the backend', async () => {
    mockCallCoreRpc.mockResolvedValue({ result: {} });

    await owner.storeSessionToken('a.b.local', { id: 'local' });

    expect(fetchMock).not.toHaveBeenCalled();
    expect(mockCallCoreRpc).toHaveBeenCalledWith({
      method: 'openhuman.auth_set_credential',
      params: { token: 'a.b.local', kind: 'local', user: { id: 'local' } },
    });
  });

  it('prefixes core failures so callers can classify them', async () => {
    mockCallCoreRpc.mockRejectedValue(new Error('core unreachable'));

    await expect(owner.storeSessionToken('a.b.local', { id: 'l' })).rejects.toThrow(
      'CORE: core unreachable'
    );
  });

  it('logs out through auth.clear_credential', async () => {
    mockCallCoreRpc.mockResolvedValue({ result: {} });
    await owner.logoutSession();
    expect(mockCallCoreRpc).toHaveBeenCalledWith({
      method: 'openhuman.auth_clear_credential',
      params: { kind: 'session' },
    });
  });

  // #6318 — an API key stored alongside the session makes `auth.get_state`
  // report `credential: 'api-key'` as the effective one (it wins over a
  // session for backend requests). Logout must still clear the hidden
  // session rather than skip it because it isn't the currently-effective
  // credential — leaving it would let clearing the API key later silently
  // restore the "logged out" session.
  it('clears the session even when an API key is the effective credential', async () => {
    mockCallCoreRpc.mockResolvedValue({ result: {} });
    await owner.logoutSession();
    expect(mockCallCoreRpc).toHaveBeenCalledWith({
      method: 'openhuman.auth_clear_credential',
      params: { kind: 'session' },
    });
    // No `auth.get_state` gate before the clear.
    expect(mockCallCoreRpc).not.toHaveBeenCalledWith(
      expect.objectContaining({ method: 'openhuman.auth_get_state' })
    );
  });

  it('serves the stored user for non-session credentials and refreshes sessions', async () => {
    mockCallCoreRpc.mockResolvedValueOnce({
      result: { isAuthenticated: true, credential: 'local', user: { id: 'l' } },
    });
    expect(await owner.fetchCurrentUser()).toEqual({
      user: { id: 'l' },
      stale: false,
      staleSeconds: null,
    });

    mockCallCoreRpc
      .mockResolvedValueOnce({
        result: { isAuthenticated: true, credential: 'session', user: { id: 'old' } },
      })
      .mockResolvedValueOnce({ result: { token: 'jwt' } })
      .mockResolvedValueOnce({ result: { token: 'jwt' } });
    fetchMock.mockResolvedValueOnce(jsonResponse(200, { success: true, user: { id: 'fresh' } }));
    expect(await owner.fetchCurrentUser()).toEqual({
      user: { id: 'fresh' },
      stale: false,
      staleSeconds: 0,
    });
  });

  it('clears the session and rethrows when /auth/me rejects the stored token', async () => {
    mockCallCoreRpc
      .mockResolvedValueOnce({
        result: { isAuthenticated: true, credential: 'session', user: null },
      })
      .mockResolvedValueOnce({ result: { token: 'jwt' } })
      .mockResolvedValueOnce({ result: { token: 'jwt' } })
      .mockResolvedValueOnce({ result: {} });
    fetchMock.mockResolvedValueOnce(jsonResponse(401, {}));
    const expired = vi.fn();
    window.addEventListener('openhuman:session-expired', expired as EventListener);

    try {
      await expect(owner.fetchCurrentUser()).rejects.toThrow(/^REJECTED:/);
    } finally {
      window.removeEventListener('openhuman:session-expired', expired as EventListener);
    }
    expect(mockCallCoreRpc).toHaveBeenLastCalledWith({
      method: 'openhuman.auth_clear_credential',
      params: { kind: 'session' },
    });
    expect(expired).toHaveBeenCalledTimes(1);
    expect((expired.mock.calls[0][0] as CustomEvent).detail).toEqual({
      source: 'browser-owner.auth/me',
      reason: 'confirmed',
    });
  });

  it('caches a fetched user per token until forced', async () => {
    const coreState = {
      result: { isAuthenticated: true, credential: 'session', user: { id: 'old' } },
    };
    const token = { result: { token: 'jwt' } };
    mockCallCoreRpc
      .mockResolvedValueOnce(coreState)
      .mockResolvedValueOnce(token)
      .mockResolvedValueOnce(token)
      .mockResolvedValueOnce(coreState)
      .mockResolvedValueOnce(token)
      .mockResolvedValueOnce(coreState)
      .mockResolvedValueOnce(token)
      .mockResolvedValueOnce(token);
    fetchMock
      .mockResolvedValueOnce(jsonResponse(200, { success: true, user: { id: 'first' } }))
      .mockResolvedValueOnce(jsonResponse(200, { success: true, user: { id: 'second' } }));

    expect((await owner.fetchCurrentUser()).user).toEqual({ id: 'first' });
    expect((await owner.fetchCurrentUser()).user).toEqual({ id: 'first' });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect((await owner.fetchCurrentUser(true)).user).toEqual({ id: 'second' });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('serves the stored user flagged stale when the backend is down', async () => {
    mockCallCoreRpc
      .mockResolvedValueOnce({
        result: { isAuthenticated: true, credential: 'session', user: { id: 'old' } },
      })
      .mockResolvedValueOnce({ result: { token: 'jwt' } });
    fetchMock.mockRejectedValueOnce(new Error('fetch failed'));

    expect(await owner.fetchCurrentUser()).toEqual({
      user: { id: 'old' },
      stale: true,
      staleSeconds: null,
    });
  });
});

describe('sessionErrorKind', () => {
  it('classifies by stable prefix', () => {
    expect(owner.sessionErrorKind('REJECTED: x')).toBe('rejected');
    expect(owner.sessionErrorKind('EXPIRED: x')).toBe('expired');
    expect(owner.sessionErrorKind('TRANSIENT: x')).toBe('transient');
    expect(owner.sessionErrorKind('CONSUME_FAILED: x')).toBe('consume_failed');
    expect(owner.sessionErrorKind('CORE: x')).toBe('core');
    expect(owner.sessionErrorKind('BACKEND: x')).toBe('core');
    expect(owner.sessionErrorKind('USER_ID_UNAVAILABLE: x')).toBe('user_id');
    expect(owner.sessionErrorKind('something else')).toBe('other');
  });
});
