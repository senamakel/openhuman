import { afterEach, describe, expect, it, vi } from 'vitest';

import reducer, { resolveTheme, setThemeMode } from './themeSlice';

describe('themeSlice', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('starts in system mode by default', () => {
    const state = reducer(undefined, { type: '@@INIT' });
    expect(state.mode).toBe('system');
  });

  it.each(['light', 'dark', 'system'] as const)('setThemeMode(%s) updates the slice', mode => {
    const state = reducer(undefined, setThemeMode(mode));
    expect(state.mode).toBe(mode);
  });

  describe('resolveTheme', () => {
    it('returns the mode verbatim for explicit light/dark', () => {
      expect(resolveTheme('light')).toBe('light');
      expect(resolveTheme('dark')).toBe('dark');
    });

    it('returns dark for system when prefers-color-scheme: dark matches', () => {
      vi.stubGlobal('window', {
        matchMedia: (query: string) => ({
          matches: query === '(prefers-color-scheme: dark)',
          media: query,
          addEventListener: () => {},
          removeEventListener: () => {},
        }),
      });
      expect(resolveTheme('system')).toBe('dark');
    });

    it('returns light for system when the dark media query does not match', () => {
      vi.stubGlobal('window', {
        matchMedia: () => ({
          matches: false,
          media: '',
          addEventListener: () => {},
          removeEventListener: () => {},
        }),
      });
      expect(resolveTheme('system')).toBe('light');
    });

    it('falls back to light when matchMedia is unavailable', () => {
      vi.stubGlobal('window', {});
      expect(resolveTheme('system')).toBe('light');
    });

    it('falls back to light when matchMedia throws', () => {
      vi.stubGlobal('window', {
        matchMedia: () => {
          throw new Error('not supported');
        },
      });
      expect(resolveTheme('system')).toBe('light');
    });
  });
});
