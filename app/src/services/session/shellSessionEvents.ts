/**
 * Bridge the Tauri shell's session-owner events into the window events the
 * core-state layer already reacts to.
 *
 * - `auth://expired` (the backend rejected the stored credential and the shell
 *   cleared it) → `openhuman:session-expired` with a `confirmed` reason, the
 *   same path the Socket.IO `auth:session_expired` push takes.
 * - `auth://changed` (credential or current user changed) → `onChanged`, so
 *   the provider refreshes without waiting for the next poll.
 */
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import { useShellSessionOwner } from './sessionOwner';

export const SHELL_AUTH_CHANGED_EVENT = 'auth://changed';
export const SHELL_AUTH_EXPIRED_EVENT = 'auth://expired';

export const installShellSessionEventBridge = (handlers: {
  onChanged: () => void;
}): (() => void) => {
  if (!useShellSessionOwner()) {
    return () => undefined;
  }
  let disposed = false;
  const unlisteners: UnlistenFn[] = [];
  const track = (promise: Promise<UnlistenFn>) => {
    promise
      .then(unlisten => {
        if (disposed) unlisten();
        else unlisteners.push(unlisten);
      })
      .catch(error => {
        console.debug('[session] shell event listener unavailable:', error);
      });
  };
  track(
    listen<{ source?: string }>(SHELL_AUTH_EXPIRED_EVENT, event => {
      window.dispatchEvent(
        new CustomEvent('openhuman:session-expired', {
          detail: { source: `shell.${event.payload?.source ?? 'unknown'}`, reason: 'confirmed' },
        })
      );
    })
  );
  track(
    listen(SHELL_AUTH_CHANGED_EVENT, () => {
      handlers.onChanged();
    })
  );
  return () => {
    disposed = true;
    for (const unlisten of unlisteners.splice(0)) unlisten();
  };
};
