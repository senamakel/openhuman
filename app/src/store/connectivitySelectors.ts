import type { HostedState } from './connectivitySlice';
import { RootState } from './index';

/**
 * Single app-level "what is broken right now?" derived state. Order matters —
 * the user-blocking outage wins over the soft "we're reconnecting" state.
 *
 * - `internet-offline`  : navigator.onLine = false. Nothing else can talk.
 * - `core-unreachable`  : local sidecar isn't answering. App is dead-in-the-water.
 * - `backend-only`      : the renderer's Socket.IO link to the core is down but
 *                         the core is alive — the app stays usable, we just
 *                         show a soft banner.
 * - `hosted-degraded`   : the core's own link to the hosted backend is down
 *                         while its loop is alive — being retried, or its
 *                         namespace closed by the server. Chat keeps working
 *                         (it rides the local core bridge); webhooks, Composio
 *                         triggers, managed-DM routing and hosted-brain runs
 *                         pause until it heals. Soft chip only (#6256).
 * - `hosted-stopped`    : that loop exited for good because the stored session
 *                         is unusable; nothing resumes until the user signs in
 *                         again. Same soft chip, different copy (#6270).
 * - `ok`                : everything healthy.
 */
type BlockingState =
  | 'internet-offline'
  | 'core-unreachable'
  | 'backend-only'
  | 'hosted-degraded'
  | 'hosted-stopped'
  | 'ok';

/**
 * A hosted link that is down: being retried (`connecting` / `reconnecting`),
 * its Socket.IO namespace closed by the server (`disconnected`), or its loop
 * stopped for good on an unusable session (`stopped`). Two states are
 * deliberately excluded: `unknown`, the loop not running at all (signed out,
 * local session, early boot) — the absence of a link, not an outage — and
 * `error`, a server `error` event on a transport that stays live and keeps
 * emitting, which is not a reconnect and must not read as one.
 */
export const isHostedDegraded = (hosted: HostedState | undefined): boolean =>
  hosted === 'connecting' ||
  hosted === 'reconnecting' ||
  hosted === 'disconnected' ||
  hosted === 'stopped';

export const selectBlockingState = (s: RootState): BlockingState => {
  if (s.connectivity.internet === 'offline') return 'internet-offline';
  if (s.connectivity.core === 'unreachable') return 'core-unreachable';
  if (s.connectivity.backend === 'disconnected' || s.connectivity.backend === 'connecting') {
    return 'backend-only';
  }
  if (s.connectivity.hosted === 'stopped') return 'hosted-stopped';
  if (isHostedDegraded(s.connectivity.hosted)) return 'hosted-degraded';
  return 'ok';
};
