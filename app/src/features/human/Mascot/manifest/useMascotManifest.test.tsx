import { configureStore } from '@reduxjs/toolkit';
import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { Provider } from 'react-redux';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import mascotReducer, { setSelectedMascotId } from '../../../../store/mascotSlice';
import type { MascotManifest, MascotManifestEntry } from './types';
import { useMascotManifest } from './useMascotManifest';

const fetchMascotManifest = vi.hoisted(() => vi.fn());
vi.mock('./manifestService', async importOriginal => {
  const actual = await importOriginal<typeof import('./manifestService')>();
  return { ...actual, fetchMascotManifest };
});

function entry(id: string, status: 'ready' | 'draft'): MascotManifestEntry {
  return {
    id,
    name: id,
    description: '',
    status,
    tags: [],
    stateEngine: {
      idlePoseCycle: ['idle'],
      states: { idle: 'idle', thinking: 'thinking' },
      visemeCodes: ['sil'],
    },
    files: [{ path: `${id}.riv`, bytes: 1, role: 'runtime', sha256: id, url: `https://x/${id}.riv` }],
  };
}

const MANIFEST: MascotManifest = {
  schemaVersion: 1,
  generatedAt: '',
  mascots: [entry('toshi', 'draft'), entry('tiny-mascot', 'ready')],
  source: { repository: '', branch: '', commit: '' },
};

function makeWrapper(selectedId: string | null) {
  const store = configureStore({ reducer: { mascot: mascotReducer } });
  if (selectedId) store.dispatch(setSelectedMascotId(selectedId));
  return ({ children }: { children: ReactNode }) => <Provider store={store}>{children}</Provider>;
}

beforeEach(() => fetchMascotManifest.mockReset());
afterEach(() => vi.restoreAllMocks());

describe('useMascotManifest', () => {
  it('resolves the selected mascot when set', async () => {
    fetchMascotManifest.mockResolvedValue(MANIFEST);
    const { result } = renderHook(() => useMascotManifest(), { wrapper: makeWrapper('toshi') });
    await waitFor(() => expect(result.current.entry?.id).toBe('toshi'));
    expect(result.current.loading).toBe(false);
  });

  it('falls back to the default (first ready) mascot when none selected', async () => {
    fetchMascotManifest.mockResolvedValue(MANIFEST);
    const { result } = renderHook(() => useMascotManifest(), { wrapper: makeWrapper(null) });
    await waitFor(() => expect(result.current.entry?.id).toBe('tiny-mascot'));
  });

  it('surfaces an error and leaves entry null when the fetch fails', async () => {
    fetchMascotManifest.mockImplementation(async () => {
      throw new Error('offline');
    });
    const { result } = renderHook(() => useMascotManifest(), { wrapper: makeWrapper(null) });
    await waitFor(() => expect(result.current.error?.message).toBe('offline'));
    expect(result.current.entry).toBeNull();
  });
});
