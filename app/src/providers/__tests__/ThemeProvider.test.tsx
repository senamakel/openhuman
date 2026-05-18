import { configureStore } from '@reduxjs/toolkit';
import { act, render } from '@testing-library/react';
import type { ReactNode } from 'react';
import { Provider } from 'react-redux';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import themeReducer, { setThemeMode, type ThemeMode } from '../../store/themeSlice';
import ThemeProvider from '../ThemeProvider';

function makeStore(mode: ThemeMode) {
  return configureStore({ reducer: { theme: themeReducer }, preloadedState: { theme: { mode } } });
}

function renderWithStore(mode: ThemeMode, children: ReactNode = null) {
  const store = makeStore(mode);
  const utils = render(
    <Provider store={store}>
      <ThemeProvider>{children}</ThemeProvider>
    </Provider>
  );
  return { ...utils, store };
}

describe('ThemeProvider', () => {
  beforeEach(() => {
    document.documentElement.classList.remove('dark');
    document.documentElement.style.colorScheme = '';
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('adds the dark class when mode=dark', () => {
    renderWithStore('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    expect(document.documentElement.style.colorScheme).toBe('dark');
  });

  it('removes the dark class when mode=light', () => {
    document.documentElement.classList.add('dark');
    renderWithStore('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
    expect(document.documentElement.style.colorScheme).toBe('light');
  });

  it('respects prefers-color-scheme when mode=system (dark match)', () => {
    const listeners = new Set<() => void>();
    vi.stubGlobal('matchMedia', (query: string) => ({
      matches: query.includes('dark'),
      media: query,
      addEventListener: (_e: string, cb: () => void) => listeners.add(cb),
      removeEventListener: (_e: string, cb: () => void) => listeners.delete(cb),
    }));
    // matchMedia is read from `window.matchMedia` — jsdom exposes it.
    (window as unknown as { matchMedia: typeof window.matchMedia }).matchMedia = (query: string) =>
      ({
        matches: query.includes('dark'),
        media: query,
        onchange: null,
        addEventListener: (_e: string, cb: EventListenerOrEventListenerObject) =>
          listeners.add(cb as () => void),
        removeEventListener: (_e: string, cb: EventListenerOrEventListenerObject) =>
          listeners.delete(cb as () => void),
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => true,
      }) as MediaQueryList;

    renderWithStore('system');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });

  it('reapplies when mode changes via the store', () => {
    const { store } = renderWithStore('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);

    act(() => {
      store.dispatch(setThemeMode('dark'));
    });
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });

  it('cleans up the prefers-color-scheme listener when unmounted (modern API)', () => {
    const remove = vi.fn();
    (window as unknown as { matchMedia: typeof window.matchMedia }).matchMedia = ((query: string) =>
      ({
        matches: false,
        media: query,
        onchange: null,
        addEventListener: () => {},
        removeEventListener: remove,
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => true,
      }) as MediaQueryList) as typeof window.matchMedia;

    const { unmount } = renderWithStore('system');
    unmount();
    expect(remove).toHaveBeenCalledTimes(1);
  });

  it('falls back to addListener/removeListener on older webviews', () => {
    const removeLegacy = vi.fn();
    (window as unknown as { matchMedia: typeof window.matchMedia }).matchMedia = ((query: string) =>
      ({
        matches: false,
        media: query,
        onchange: null,
        // addEventListener intentionally omitted to force the legacy branch.
        addEventListener: undefined as unknown as MediaQueryList['addEventListener'],
        removeEventListener: () => {},
        addListener: () => {},
        removeListener: removeLegacy,
        dispatchEvent: () => true,
      }) as MediaQueryList) as typeof window.matchMedia;

    const { unmount } = renderWithStore('system');
    unmount();
    expect(removeLegacy).toHaveBeenCalledTimes(1);
  });
});
