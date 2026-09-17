/**
 * coreHealthMonitor — polls the local Rust sidecar's `openhuman.connectivity_diag`
 * endpoint and dispatches `setCore` to the connectivitySlice (#1527).
 *
 * The same reply carries the core's own link to the hosted backend
 * (`socket_state`), which is mirrored into the `hosted` channel so the
 * connectivity chip can show an outage of that link (#6256). It is a poll, so
 * a drop that heals within one healthy interval is never shown — deliberately:
 * a two-second blip is not actionable and a flapping chip would be noise. A
 * drop that persists appears within one healthy interval and clears within
 * one degraded interval, because a degraded hosted link also selects the fast
 * cadence.
 *
 * Polling cadence is adaptive:
 *   - healthy   : 30s (cheap heartbeat)
 *   - degraded  : 5s (fast recovery detection)
 *
 * A single transient failure is not enough to flip the channel — we require
 * `FAIL_THRESHOLD` consecutive failures to mark `unreachable` so a single
 * dropped TCP packet doesn't pop a scary blocking screen.
 */
import { isHostedDegraded } from '../store/connectivitySelectors';
import { type HostedState, setCore, setHosted } from '../store/connectivitySlice';
import { store } from '../store/index';
import { callCoreRpc } from './coreRpcClient';

const HEALTHY_INTERVAL_MS = 30_000;
const DEGRADED_INTERVAL_MS = 5_000;
const FAIL_THRESHOLD = 2;

/** The core's `ConnectionStatus` values the hosted channel mirrors verbatim. */
const HOSTED_STATES: ReadonlySet<string> = new Set<HostedState>([
  'connected',
  'connecting',
  'reconnecting',
  'disconnected',
  'error',
]);

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' ? (value as Record<string, unknown>) : null;
}

/**
 * Peel a `connectivity_diag` reply down to the diag payload. The handler
 * answers `{ diag: { socket_state, … } }`, and because it attaches a log line
 * the RPC layer wraps that again as `{ result: { diag }, logs }` (the
 * log-envelope rule, #6080). Both layers are removed here and a bare payload
 * is accepted too, so a future normalisation of that rule cannot silently
 * turn every reading into `unknown`.
 */
function unwrapDiag(reply: unknown): Record<string, unknown> {
  let record = asRecord(reply) ?? {};
  const result = asRecord(record.result);
  if (result) record = result;
  const diag = asRecord(record.diag);
  if (diag) record = diag;
  return record;
}

/**
 * Map a `connectivity_diag` reply onto the hosted channel.
 *
 * The core says whether anyone is retrying: `socket_loop_active` is `false`
 * when its reconnect loop is not running (never connected, signed out, local
 * session), and that is `unknown` here — not an outage — whatever
 * `socket_state` says. The one exception is a loop that stopped for good on
 * an unusable session (`socket_loop_stopped_on_failure`), which is `stopped`:
 * integrations and channels are off until the user signs in again, and the
 * chip must say so. While the loop is alive the status passes through
 * verbatim, including `disconnected`, which then means the server closed the
 * Socket.IO namespace on a transport it left open. A reply with no
 * `socket_state` is `unknown` too.
 *
 * A core that predates `socket_loop_active` cannot say who is retrying, so
 * its `disconnected` — most often a loop that is not running — stays
 * `unknown` rather than raising a false outage; every other status is taken
 * as is, because the loop reports `reconnecting` between retries.
 */
export function hostedStateFromDiag(reply: unknown): { value: HostedState; error?: string } {
  const record = unwrapDiag(reply);
  const socketState = record.socket_state;
  const loopActive = record.socket_loop_active;
  const stoppedOnFailure = record.socket_loop_stopped_on_failure;
  const lastError = record.last_ws_error;
  let value: HostedState = 'unknown';
  if (loopActive === false) {
    if (stoppedOnFailure === true) value = 'stopped';
  } else if (
    typeof socketState === 'string' &&
    HOSTED_STATES.has(socketState) &&
    !(loopActive === undefined && socketState === 'disconnected')
  ) {
    value = socketState as HostedState;
  }
  return typeof lastError === 'string' && lastError.length > 0
    ? { value, error: lastError }
    : { value };
}

let timer: ReturnType<typeof setTimeout> | null = null;
let consecutiveFails = 0;
let stopped = true;

async function probe(): Promise<void> {
  try {
    const diag = await callCoreRpc<unknown>({ method: 'openhuman.connectivity_diag', params: {} });
    consecutiveFails = 0;
    store.dispatch(setCore({ value: 'reachable' }));
    store.dispatch(setHosted(hostedStateFromDiag(diag)));
  } catch (err) {
    consecutiveFails += 1;
    const message = err instanceof Error ? err.message : String(err);
    if (consecutiveFails >= FAIL_THRESHOLD) {
      store.dispatch(setCore({ value: 'unreachable', error: message }));
    }
  } finally {
    if (!stopped) schedule();
  }
}

function schedule(): void {
  if (timer != null) clearTimeout(timer);
  // Use the failure streak (not just the Redux state) so we enter degraded
  // 5s polling on the *first* miss — before the threshold flips `core` to
  // `unreachable`. Without this, first-failure retries stayed at 30s.
  // (addresses @coderabbitai on coreHealthMonitor.ts:46)
  // A degraded hosted link selects the fast cadence too, so its recovery is
  // reflected within seconds rather than at the next 30s heartbeat (#6256).
  // `error` is not degraded (live transport), so it never pins the fast poll.
  const { core, hosted } = store.getState().connectivity;
  const isDegraded = consecutiveFails > 0 || core !== 'reachable' || isHostedDegraded(hosted);
  const interval = isDegraded ? DEGRADED_INTERVAL_MS : HEALTHY_INTERVAL_MS;
  timer = setTimeout(() => void probe(), interval);
}

export function startCoreHealthMonitor(): void {
  if (!stopped) return;
  stopped = false;
  consecutiveFails = 0;
  void probe();
}

export function stopCoreHealthMonitor(): void {
  stopped = true;
  if (timer != null) {
    clearTimeout(timer);
    timer = null;
  }
}
