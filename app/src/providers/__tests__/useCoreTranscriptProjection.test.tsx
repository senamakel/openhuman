import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { threadApi } from '../../services/api/threadApi';
import type { DerivedDisplayItem, DerivedTranscriptPage } from '../../types/derivedTranscript';
import { useCoreTranscriptProjection } from '../useOpenHumanExternalStore';

vi.mock('../../services/api/threadApi', () => ({ threadApi: { getDerivedTranscript: vi.fn() } }));

const THREAD = 'thread-long';

/** Newest-first items for one settled turn: a tool call after its boundary. */
function turn(requestId: string, callId: string): DerivedDisplayItem[] {
  return [
    { kind: 'toolCall', callId, name: 'web_search_tool', status: 'success', result: 'ok' },
    { kind: 'assistantMessage', content: `answer ${requestId}`, requestId, iteration: 1 },
    { kind: 'turnBoundary', requestId },
  ];
}

function page(over: Partial<DerivedTranscriptPage>): DerivedTranscriptPage {
  return { threadId: THREAD, items: [], total: 0, hasMore: false, hasTranscript: true, ...over };
}

describe('useCoreTranscriptProjection paging', () => {
  beforeEach(() => {
    vi.mocked(threadApi.getDerivedTranscript).mockReset();
  });

  it('walks every older page and projects the whole history', async () => {
    vi.mocked(threadApi.getDerivedTranscript)
      .mockResolvedValueOnce(
        page({ items: turn('r-new', 'call-new'), total: 6, hasMore: true, nextCursor: 'c1' })
      )
      .mockResolvedValueOnce(page({ items: turn('r-old', 'call-old'), total: 6, hasMore: false }));

    const { result } = renderHook(() => useCoreTranscriptProjection(THREAD, 'rev-1', undefined));

    await waitFor(() => expect(Object.keys(result.current.timelines)).toHaveLength(2));
    expect(threadApi.getDerivedTranscript).toHaveBeenCalledTimes(2);
    expect(threadApi.getDerivedTranscript).toHaveBeenLastCalledWith(THREAD, {
      limit: 500,
      cursor: 'c1',
    });
    expect(result.current.timelines['r-old']?.[0]).toMatchObject({ id: 'call-old' });
    expect(result.current.timelines['r-new']?.[0]).toMatchObject({ id: 'call-new' });
  });

  it('stops when the core reports no more pages', async () => {
    vi.mocked(threadApi.getDerivedTranscript).mockResolvedValueOnce(
      page({ items: turn('r-only', 'call-only'), total: 3, hasMore: false })
    );

    const { result } = renderHook(() => useCoreTranscriptProjection(THREAD, 'rev-1', undefined));

    await waitFor(() => expect(Object.keys(result.current.timelines)).toEqual(['r-only']));
    expect(threadApi.getDerivedTranscript).toHaveBeenCalledTimes(1);
  });

  it('drops a page that lands after the thread changed', async () => {
    const older = new Promise<DerivedTranscriptPage>(() => {
      /* never resolves: the walk is still in flight when the thread switches */
    });
    vi.mocked(threadApi.getDerivedTranscript)
      .mockResolvedValueOnce(
        page({ items: turn('r-new', 'call-new'), total: 6, hasMore: true, nextCursor: 'c1' })
      )
      .mockReturnValueOnce(older);

    const { result, rerender } = renderHook(
      ({ thread }) => useCoreTranscriptProjection(thread, 'rev-1', undefined),
      { initialProps: { thread: THREAD as string | null } }
    );
    await waitFor(() => expect(Object.keys(result.current.timelines)).toEqual(['r-new']));

    rerender({ thread: null });
    expect(result.current.timelines).toEqual({});
  });
});

describe('useCoreTranscriptProjection identity', () => {
  beforeEach(() => {
    vi.mocked(threadApi.getDerivedTranscript).mockReset();
  });

  /**
   * The projection refetches several times per turn (every change of the last
   * message or the lifecycle). Minting fresh arrays for every turn each time
   * missed the settled-message conversion cache for the whole thread and gave
   * assistant-ui new part objects for turns nothing had happened to.
   */
  it('keeps the same arrays for turns a refetch did not change', async () => {
    const both = [...turn('r-2', 'call-2'), ...turn('r-1', 'call-1')];
    vi.mocked(threadApi.getDerivedTranscript).mockResolvedValue(
      page({ items: both, total: both.length })
    );

    const { result, rerender } = renderHook(
      ({ revision }) => useCoreTranscriptProjection(THREAD, revision, undefined),
      { initialProps: { revision: 'rev-1' } }
    );
    await waitFor(() => expect(Object.keys(result.current.timelines)).toHaveLength(2));
    const first = result.current;

    // A refetch that returns the same turns hands back the same projection.
    rerender({ revision: 'rev-2' });
    await waitFor(() => expect(threadApi.getDerivedTranscript).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.current).toBe(first));

    // A refetch that changes ONE turn keeps the other turn's arrays.
    const changed = [
      { kind: 'toolCall', callId: 'call-3', name: 'shell', status: 'success' },
      ...both,
    ] satisfies DerivedDisplayItem[];
    vi.mocked(threadApi.getDerivedTranscript).mockResolvedValue(
      page({ items: changed, total: changed.length })
    );
    rerender({ revision: 'rev-3' });
    await waitFor(() => expect(result.current.timelines['r-2']).toHaveLength(2));
    expect(result.current.timelines['r-1']).toBe(first.timelines['r-1']);
    expect(result.current.timelines['r-2']).not.toBe(first.timelines['r-2']);
  });
});

describe('history refresh stability', () => {
  it('keeps loaded trails visible when a next-turn refetch fails', async () => {
    vi.mocked(threadApi.getDerivedTranscript)
      .mockReset()
      .mockResolvedValueOnce(page({ items: turn('old', 'old-call') }))
      .mockRejectedValueOnce(new Error('temporary RPC failure'));
    const { result, rerender } = renderHook(
      ({ revision }) => useCoreTranscriptProjection(THREAD, revision, undefined),
      { initialProps: { revision: 'before' } }
    );
    await waitFor(() => expect(result.current.timelines.old).toHaveLength(1));
    const previous = result.current;
    await act(async () => rerender({ revision: 'try-now' }));
    expect(result.current).toBe(previous);
  });

  it('does not erase older cards while paging a next-turn refresh', async () => {
    let resolveOlder!: (value: DerivedTranscriptPage) => void;
    const older = new Promise<DerivedTranscriptPage>(resolve => {
      resolveOlder = resolve;
    });
    vi.mocked(threadApi.getDerivedTranscript)
      .mockReset()
      .mockResolvedValueOnce(
        page({ items: [...turn('new', 'new-call'), ...turn('old', 'old-call')] })
      )
      .mockResolvedValueOnce(
        page({ items: turn('new', 'new-call'), hasMore: true, nextCursor: 'older' })
      )
      .mockReturnValueOnce(older);
    const { result, rerender } = renderHook(
      ({ revision }) => useCoreTranscriptProjection(THREAD, revision, undefined),
      { initialProps: { revision: 'before' } }
    );
    await waitFor(() => expect(result.current.timelines.old).toHaveLength(1));
    const old = result.current.timelines.old;
    await act(async () => rerender({ revision: 'try-now' }));
    await waitFor(() => expect(threadApi.getDerivedTranscript).toHaveBeenCalledTimes(3));
    expect(result.current.timelines.old).toBe(old);
    await act(async () => resolveOlder(page({ items: turn('old', 'old-call') })));
    expect(result.current.timelines.old).toBe(old);
  });
});
