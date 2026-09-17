import { listen } from '@tauri-apps/api/event';
import { beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

import { useShellSessionOwner } from '../sessionOwner';
import { installShellSessionEventBridge } from '../shellSessionEvents';

const mockListen = listen as Mock;

describe('installShellSessionEventBridge', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('does nothing when the shell is not the session owner', () => {
    vi.mocked(useShellSessionOwner).mockReturnValue(false);
    const dispose = installShellSessionEventBridge({ onChanged: vi.fn() });
    expect(mockListen).not.toHaveBeenCalled();
    dispose();
  });

  it('re-dispatches auth://expired as a confirmed session-expired window event', async () => {
    vi.mocked(useShellSessionOwner).mockReturnValue(true);
    const handlers = new Map<string, (event: { payload: unknown }) => void>();
    const unlisten = vi.fn();
    mockListen.mockImplementation(
      async (name: string, handler: (event: { payload: unknown }) => void) => {
        handlers.set(name, handler);
        return unlisten;
      }
    );
    const onChanged = vi.fn();
    const expired = vi.fn();
    window.addEventListener('openhuman:session-expired', expired);

    const dispose = installShellSessionEventBridge({ onChanged });
    await Promise.resolve();
    await Promise.resolve();

    handlers.get('auth://expired')?.({ payload: { source: 'auth/me' } });
    expect(expired).toHaveBeenCalledTimes(1);
    expect((expired.mock.calls[0][0] as CustomEvent).detail).toEqual({
      source: 'shell.auth/me',
      reason: 'confirmed',
    });

    handlers.get('auth://changed')?.({ payload: {} });
    expect(onChanged).toHaveBeenCalledTimes(1);

    dispose();
    expect(unlisten).toHaveBeenCalledTimes(2);
    window.removeEventListener('openhuman:session-expired', expired);
  });
});
