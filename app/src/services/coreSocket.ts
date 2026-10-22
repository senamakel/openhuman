/**
 * Shared Socket.IO factory for connections to the local OpenHuman core
 * (the in-process Rust server, addressed at `getCoreHttpBaseUrl()` or
 * the user's cloud-mode override).
 *
 * The core handshake validates the per-process bearer token, so every
 * caller has to read it via `getCoreRpcToken()` and pass it through
 * `io(url, { auth: { token } })`. Centralising the factory keeps the
 * handshake shape uniform at the current `socketService` call site and gives
 * it a single line to call.
 */
import { io, type ManagerOptions, type Socket, type SocketOptions } from 'socket.io-client';

import { getCoreRpcToken, resolveCoreSocketEndpoint } from './coreRpcClient';

interface CoreSocketOptions {
  /**
   * Per-process core bearer (from `getCoreRpcToken()`). When `null` the
   * factory passes an empty string — the server will reject the
   * handshake, but tests that mock `io` need not bother priming the
   * token resolver.
   */
  coreToken: string | null;
  /**
   * Extra fields merged onto the `auth` payload, or a callback that builds
   * fresh fields for each namespace connection (used for short-lived SaaS
   * socket tickets).
   */
  authExtras?:
    | Record<string, unknown>
    | ((callback: (data: Record<string, unknown>) => void) => void);
  /**
   * Override of the underlying Socket.IO connect options. The default
   * shape matches what the previous in-line callers used.
   */
  overrides?: Partial<ManagerOptions & SocketOptions>;
}

const DEFAULT_OPTIONS: Partial<ManagerOptions & SocketOptions> = {
  path: '/socket.io/',
  transports: ['websocket', 'polling'],
  reconnection: true,
  reconnectionDelay: 2000,
  reconnectionAttempts: Infinity,
  forceNew: true,
};

export function createCoreSocket(baseUrl: string, opts: CoreSocketOptions): Socket {
  const auth =
    typeof opts.authExtras === 'function'
      ? (callback: (data: Record<string, unknown>) => void) =>
          opts.authExtras?.((data) => callback({ token: opts.coreToken ?? '', ...data }))
      : { token: opts.coreToken ?? '', ...(opts.authExtras ?? {}) };
  return io(baseUrl, { ...DEFAULT_OPTIONS, ...(opts.overrides ?? {}), auth });
}

interface ConnectCoreSocketOptions {
  /** Resolves the Socket.IO base URL (no trailing `/rpc`). */
  getBaseUrl: () => Promise<string>;
  /**
   * Caller's disposal flag. Awaited points (`getBaseUrl`, `getCoreRpcToken`)
   * check this and short-circuit so the React effect can race a teardown
   * without leaking a connection.
   */
  isDisposed?: () => boolean;
  authExtras?:
    | Record<string, unknown>
    | ((callback: (data: Record<string, unknown>) => void) => void);
  overrides?: Partial<ManagerOptions & SocketOptions>;
}

/**
 * Resolve the base URL + core bearer, then hand off to `createCoreSocket`.
 *
 * Returns `null` if the caller's `isDisposed` flag flips during an await
 * point — the caller does not need to also wrap the call in a disposed
 * check. Keeps the per-callsite plumbing to a single line so the only
 * thing the call sites need to test is "did the helper get invoked".
 */
export async function connectCoreSocket(opts: ConnectCoreSocketOptions): Promise<Socket | null> {
  const isDisposed = opts.isDisposed ?? (() => false);
  const baseUrl = await opts.getBaseUrl();
  if (isDisposed()) return null;
  const coreToken = await getCoreRpcToken();
  if (isDisposed()) return null;
  const endpoint = await resolveCoreSocketEndpoint(baseUrl);
  if (isDisposed()) return null;
  return createCoreSocket(endpoint.baseUrl, {
    coreToken,
    authExtras: opts.authExtras,
    overrides: {
      ...opts.overrides,
      path: endpoint.transports ? endpoint.path : (opts.overrides?.path ?? endpoint.path),
      ...(endpoint.transports ? { transports: endpoint.transports } : {}),
    },
  });
}
