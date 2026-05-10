import { getVersion } from '@tauri-apps/api/app';
import { isTauri } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/app', () => ({ getVersion: vi.fn() }));

describe('apiClient version headers', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.mocked(isTauri).mockReturnValue(false);
    vi.stubGlobal('fetch', vi.fn());
  });

  it('adds x-web-version on non-Tauri backend requests', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValueOnce({
      ok: true,
      headers: new Headers({ 'content-type': 'application/json' }),
      json: async () => ({ success: true }),
    } as Response);

    const { apiClient } = await import('../apiClient');
    await apiClient.get('/version-check', { requireAuth: false });

    const requestInit = fetchMock.mock.calls[0][1] as RequestInit;
    const headers = requestInit.headers as Record<string, string>;
    expect(headers['x-web-version']).toBe('0.0.0-test');
    expect(headers).not.toHaveProperty('x-tauri-version');
  });

  it('adds sanitized x-tauri-version on Tauri backend requests', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    vi.mocked(getVersion).mockResolvedValue(' 1.2.3 (desktop)+build!? ');

    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValueOnce({
      ok: true,
      headers: new Headers({ 'content-type': 'application/json' }),
      json: async () => ({ success: true }),
    } as Response);

    const { apiClient } = await import('../apiClient');
    await apiClient.post('/version-check', { ok: true }, { requireAuth: false });

    const requestInit = fetchMock.mock.calls[0][1] as RequestInit;
    const headers = requestInit.headers as Record<string, string>;
    expect(headers['x-tauri-version']).toBe('1.2.3desktop+build');
    expect(headers).not.toHaveProperty('x-web-version');
  });

  it('retries tauri version lookup after a transient failure', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    vi.mocked(getVersion)
      .mockRejectedValueOnce(new Error('transient failure'))
      .mockResolvedValueOnce('2.3.4');

    const { getClientVersionHeaders } = await import('../clientVersionHeaders');

    await expect(getClientVersionHeaders()).resolves.toEqual({});
    await expect(getClientVersionHeaders()).resolves.toEqual({ 'x-tauri-version': '2.3.4' });
    expect(getVersion).toHaveBeenCalledTimes(2);
  });
});
