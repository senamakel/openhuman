import { renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AGENT_ACCOUNT_ID } from '../../../utils/accountsFullscreen';
import { useNewChat } from './useNewChat';

// Drive the real hook body while stubbing its router + store dependencies so
// each branch (reuse-empty-thread, create-new-thread) runs deterministically
// without a live core RPC.
const mockNavigate = vi.fn();
vi.mock('react-router-dom', async importOriginal => {
  const actual = await importOriginal<typeof import('react-router-dom')>();
  return { ...actual, useNavigate: () => mockNavigate };
});

interface MockThread {
  id: string;
  messageCount: number;
}

interface MockAction {
  type?: string;
}

interface WrapperProps {
  children: ReactNode;
}

const mockDispatch = vi.fn();
let mockThreads: MockThread[] = [];
vi.mock('../../../store/hooks', () => ({
  useAppDispatch: () => mockDispatch,
  useAppSelector: (sel: (s: unknown) => unknown) => sel({ thread: { threads: mockThreads } }),
}));

vi.mock('../../../store/accountsSlice', () => ({
  setActiveAccount: vi.fn((id: string) => ({ type: 'accounts/setActiveAccount', payload: id })),
}));
vi.mock('../../../store/threadSlice', () => ({
  createNewThread: vi.fn(() => ({ type: 'thread/createNewThread' })),
  loadThreadMessages: vi.fn((id: string) => ({ type: 'thread/loadThreadMessages', payload: id })),
  setSelectedThread: vi.fn((id: string) => ({ type: 'thread/setSelectedThread', payload: id })),
}));

function wrapper({ children }: WrapperProps) {
  return <MemoryRouter>{children}</MemoryRouter>;
}

function dispatchedTypes() {
  return mockDispatch.mock.calls.map(([action]) => action?.type);
}

describe('useNewChat', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockThreads = [];
    mockDispatch.mockImplementation((action: MockAction) => {
      if (action?.type === 'thread/createNewThread') {
        return { unwrap: () => Promise.resolve({ id: 'fresh-thread' }) };
      }
      return undefined;
    });
  });

  it('always switches to the agent account (never reopens a connected-app webview)', () => {
    const { result } = renderHook(() => useNewChat(), { wrapper });
    result.current();
    expect(mockDispatch).toHaveBeenCalledWith({
      type: 'accounts/setActiveAccount',
      payload: AGENT_ACCOUNT_ID,
    });
  });

  it('reuses an existing empty thread regardless of route (does not just /chat)', () => {
    mockThreads = [{ id: 'empty-1', messageCount: 0 }];
    const { result } = renderHook(() => useNewChat(), { wrapper });
    result.current();

    // The key regression: it selects a blank thread and navigates straight to it
    // instead of navigating to bare '/chat' (which would restore the persisted
    // conversation).
    expect(mockNavigate).toHaveBeenCalledWith('/chat/empty-1');
    expect(mockNavigate).not.toHaveBeenCalledWith('/chat');
    expect(mockDispatch).toHaveBeenCalledWith({
      type: 'thread/setSelectedThread',
      payload: 'empty-1',
    });
    expect(dispatchedTypes()).not.toContain('thread/createNewThread');
  });

  it('creates a new thread when there is no empty thread', async () => {
    mockThreads = [{ id: 't-busy', messageCount: 4 }];
    const { result } = renderHook(() => useNewChat(), { wrapper });
    result.current();

    expect(dispatchedTypes()).toContain('thread/createNewThread');
    await waitFor(() => {
      expect(mockDispatch).toHaveBeenCalledWith({
        type: 'thread/setSelectedThread',
        payload: 'fresh-thread',
      });
    });
    expect(mockDispatch).toHaveBeenCalledWith({
      type: 'thread/loadThreadMessages',
      payload: 'fresh-thread',
    });
    expect(mockNavigate).toHaveBeenCalledWith('/chat/fresh-thread');
  });

  it('swallows a failed thread creation without throwing', async () => {
    mockThreads = [{ id: 't-busy', messageCount: 2 }];
    mockDispatch.mockImplementation((action: MockAction) => {
      if (action?.type === 'thread/createNewThread') {
        return { unwrap: () => Promise.reject(new Error('boom')) };
      }
      return undefined;
    });
    const { result } = renderHook(() => useNewChat(), { wrapper });
    expect(() => result.current()).not.toThrow();
    await Promise.resolve();
    expect(dispatchedTypes()).toContain('thread/createNewThread');
  });
});
