import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { clearRivMemoryCache, loadRivBuffer } from './rivCache';

function riv(byte = 1): ArrayBuffer {
  return new Uint8Array([0x52, 0x49, 0x56, 0x45, byte]).buffer; // "RIVE" + tag
}

describe('rivCache.loadRivBuffer (version-keyed)', () => {
  beforeEach(() => {
    clearRivMemoryCache();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  function mockFetch(buffer: ArrayBuffer) {
    const fn = vi
      .fn()
      .mockResolvedValue({
        ok: true,
        arrayBuffer: () => Promise.resolve(buffer),
      } as unknown as Response);
    vi.stubGlobal('fetch', fn);
    return fn;
  }

  it('fetches once, then serves the same version from cache', async () => {
    const fetchFn = mockFetch(riv(1));

    const a = await loadRivBuffer('toshi', '1.0.0', 'http://x/mascots/toshi/riv?v=1.0.0');
    const b = await loadRivBuffer('toshi', '1.0.0', 'http://x/mascots/toshi/riv?v=1.0.0');

    expect(new Uint8Array(a)[0]).toBe(0x52);
    expect(b).toBe(a); // identical cached buffer
    expect(fetchFn).toHaveBeenCalledTimes(1); // no second network hit
  });

  it('re-fetches when the version changes', async () => {
    const fetchFn = mockFetch(riv(1));
    await loadRivBuffer('toshi', '1.0.0', 'http://x/riv?v=1.0.0');
    expect(fetchFn).toHaveBeenCalledTimes(1);

    await loadRivBuffer('toshi', '2.0.0', 'http://x/riv?v=2.0.0');
    expect(fetchFn).toHaveBeenCalledTimes(2); // version bump invalidates cache
  });

  it('throws on a non-OK response', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({ ok: false, status: 404 } as unknown as Response)
    );
    await expect(loadRivBuffer('missing', '1', 'http://x/riv')).rejects.toThrow(/404/);
  });
});
