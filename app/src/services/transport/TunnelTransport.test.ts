/**
 * Unit tests for TunnelTransport.
 *
 * We mock socket.io-client so no real network connection is made.
 * Each test gets a fresh socket mock via the module factory pattern.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  base64urlEncode,
  deriveSharedSecret,
  generateKeypair,
  open,
  ReplayTracker,
  seal,
} from '../../lib/tunnel/crypto';

// -- socket mock factory -------------------------------------------------------

// The mock must be registered before the module under test is imported, but
// we need fresh state per test. We use module-level mutable objects the
// factory closure captures.

let _handlers: Map<string, (...args: unknown[]) => void> = new Map();
let _emitSpy = vi.fn();
let _disconnectSpy = vi.fn();

vi.mock('socket.io-client', () => ({
  io: () => ({
    on: (event: string, cb: (...args: unknown[]) => void) => {
      _handlers.set(event, cb);
    },
    emit: (...args: unknown[]) => _emitSpy(...args),
    disconnect: () => _disconnectSpy(),
    connected: true,
  }),
}));

// Import AFTER vi.mock is hoisted.
const { TunnelTransport } = await import('./TunnelTransport');

// -- helpers ------------------------------------------------------------------

function resetSocket() {
  _handlers = new Map();
  _emitSpy = vi.fn();
  _disconnectSpy = vi.fn();
}

function fire(event: string, ...args: unknown[]) {
  _handlers.get(event)?.(...args);
}

async function connectTransport(transport: InstanceType<typeof TunnelTransport>): Promise<void> {
  const connectP = (transport as unknown as { ensureConnected(): Promise<void> }).ensureConnected();
  // Flush: give socket.on a chance to register.
  await Promise.resolve();
  fire('connect');
  await Promise.resolve();
  fire('tunnel:connected');
  await connectP;
}

function coreB64(kp: ReturnType<typeof generateKeypair>) {
  return base64urlEncode(kp.publicKey);
}

// -- tests --------------------------------------------------------------------

beforeEach(() => {
  resetSocket();
});

describe('TunnelTransport', () => {
  it('emits tunnel:connect with channelId + role on connect', async () => {
    const coreKp = generateKeypair();
    const channelId = 'CHAN_001';
    const transport = new TunnelTransport('http://backend', channelId, coreB64(coreKp), 'tok');

    await connectTransport(transport);

    const connectCall = _emitSpy.mock.calls.find(([ev]) => ev === 'tunnel:connect');
    expect(connectCall).toBeTruthy();
    expect(connectCall![1]).toMatchObject({ channelId, role: 'client', token: 'tok' });

    // Handshake frame should have been sent.
    const frameCall = _emitSpy.mock.calls.find(([ev]) => ev === 'tunnel:frame');
    expect(frameCall).toBeTruthy();

    await transport.close();
  });

  it('rejects pending calls when close() is called', async () => {
    const coreKp = generateKeypair();
    const transport = new TunnelTransport('http://backend', 'CHAN_002', coreB64(coreKp), 'tok');

    await connectTransport(transport);

    // Queue a call.
    const callP = transport.call('openhuman.ping', {});

    // Close immediately — pending call should reject.
    await transport.close();

    await expect(callP).rejects.toThrow();
  }, 5000);

  it('replay rejection: duplicate encrypted frames are rejected', () => {
    const kp = generateKeypair();
    const other = generateKeypair();
    const key = deriveSharedSecret(kp.secretKey, other.publicKey);
    const tracker = new ReplayTracker();

    const plain = new TextEncoder().encode(
      '{"requestId":"r1","kind":"response","seq":0,"payload":null}'
    );
    const frame = seal(key, plain);

    // First open: ok.
    const first = open(key, frame, tracker);
    expect(Array.from(first)).toEqual(Array.from(plain));

    // Second open of same frame: replayed nonce.
    expect(() => open(key, frame, tracker)).toThrow(/replayed nonce/i);
  });

  it('rejects the connect promise on tunnel:error', async () => {
    const coreKp = generateKeypair();
    const transport = new TunnelTransport('http://backend', 'CHAN_003', coreB64(coreKp), 'tok');

    const connectP = (
      transport as unknown as { ensureConnected(): Promise<void> }
    ).ensureConnected();
    await Promise.resolve();
    fire('connect');
    await Promise.resolve();
    // Fire tunnel:error instead of tunnel:connected.
    fire('tunnel:error', 'unauthorized');

    await expect(connectP).rejects.toThrow(/server error|unauthorized/i);
  }, 5000);
});
