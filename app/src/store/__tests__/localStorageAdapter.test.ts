// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { localStorageAdapter } from '../localStorageAdapter';

describe('localStorageAdapter', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('getItem', () => {
    it('returns the stored value when present', async () => {
      localStorage.setItem('foo', 'bar');
      await expect(localStorageAdapter.getItem('foo')).resolves.toBe('bar');
    });

    it('returns null when the key is missing', async () => {
      await expect(localStorageAdapter.getItem('missing')).resolves.toBeNull();
    });

    it('returns null and swallows the error when localStorage.getItem throws', async () => {
      const spy = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
        throw new Error('SecurityError: private mode');
      });
      await expect(localStorageAdapter.getItem('foo')).resolves.toBeNull();
      expect(spy).toHaveBeenCalledWith('foo');
    });
  });

  describe('setItem', () => {
    it('writes the value through to localStorage', async () => {
      await localStorageAdapter.setItem('foo', 'bar');
      expect(localStorage.getItem('foo')).toBe('bar');
    });

    it('resolves without throwing when localStorage.setItem throws (e.g. quota)', async () => {
      const spy = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
        throw new Error('QuotaExceededError');
      });
      await expect(localStorageAdapter.setItem('foo', 'bar')).resolves.toBeUndefined();
      expect(spy).toHaveBeenCalledWith('foo', 'bar');
    });
  });

  describe('removeItem', () => {
    it('removes the value from localStorage', async () => {
      localStorage.setItem('foo', 'bar');
      await localStorageAdapter.removeItem('foo');
      expect(localStorage.getItem('foo')).toBeNull();
    });

    it('resolves without throwing when localStorage.removeItem throws', async () => {
      const spy = vi.spyOn(Storage.prototype, 'removeItem').mockImplementation(() => {
        throw new Error('SecurityError');
      });
      await expect(localStorageAdapter.removeItem('foo')).resolves.toBeUndefined();
      expect(spy).toHaveBeenCalledWith('foo');
    });
  });

  it('exposes the redux-persist Storage shape (3 async methods)', () => {
    expect(typeof localStorageAdapter.getItem).toBe('function');
    expect(typeof localStorageAdapter.setItem).toBe('function');
    expect(typeof localStorageAdapter.removeItem).toBe('function');
    // Each call returns a thenable so redux-persist's await machinery
    // works regardless of underlying success/throw.
    expect(localStorageAdapter.getItem('x')).toBeInstanceOf(Promise);
    expect(localStorageAdapter.setItem('x', 'y')).toBeInstanceOf(Promise);
    expect(localStorageAdapter.removeItem('x')).toBeInstanceOf(Promise);
  });
});
