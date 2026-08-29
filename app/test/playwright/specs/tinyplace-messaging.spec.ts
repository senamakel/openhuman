// Core-RPC e2e: tiny.place direct messaging between the app's own core (Alice)
// and a second real openhuman-core (Bob), against a real tiny.place backend.
//
// This used to also drive the in-app Messaging UI (the `/agent-world/messaging`
// route), but that surface was removed (#5424) along with the rest of the
// Agent World / tiny.place UI. What remains here is the part with independent
// value: proving that the app's own bootstrapped core (the one the web-session
// harness launches, PW_CORE_RPC_URL) can complete a full encrypted DM round
// trip against a real peer core and a real backend — not just the manually
// launched pair the core suite at e2e/tinyplace-messaging/messaging.e2e.mjs
// already exercises exhaustively (contact request/accept, refusal between
// non-contacts, ciphertext-only relay, ratchet reply, etc. — not re-asserted
// here to avoid duplicating that suite).
//
// Requires the web session harness (app/scripts/e2e-web-session.sh) with
// TINYPLACE_API_BASE_URL exported so the core hits a real backend. The
// messaging e2e runner (e2e/tinyplace-messaging/run-ui.sh) wires this up.
import { expect, test } from '@playwright/test';

// The core-launch helper is shared with the core-level suite (plain ESM).
import { launchAgent, receiveMessage } from '../../../../e2e/tinyplace-messaging/lib/core.mjs';

const CORE_RPC_URL = process.env.PW_CORE_RPC_URL || 'http://127.0.0.1:17788/rpc';
const CORE_RPC_TOKEN = process.env.PW_CORE_RPC_TOKEN || 'openhuman-playwright-token';
const BACKEND = process.env.TINYPLACE_API_BASE_URL || 'http://localhost:18080';
const HAS_TINYPLACE_BACKEND = Boolean(process.env.TINYPLACE_API_BASE_URL);

const TEST_MNEMONIC_WORDS = 12;
// A fresh, valid BIP-39 mnemonic for Alice (the app's core identity). Generated
// via the same dependency-free generator the core suite uses.
async function freshMnemonic(): Promise<string> {
  const { generateMnemonic } = await import('../../../../e2e/tinyplace-messaging/lib/mnemonic.mjs');
  const m = generateMnemonic();
  if (m.split(' ').length !== TEST_MNEMONIC_WORDS) throw new Error('unexpected mnemonic length');
  return m;
}

const PLACEHOLDER_ACCOUNTS = [
  {
    chain: 'evm',
    address: '0x0000000000000000000000000000000000000001',
    derivationPath: "m/44'/60'/0'/0/0",
  },
  {
    chain: 'btc',
    address: 'bc1qplaceholderplaceholderplaceholderplac0000',
    derivationPath: "m/84'/0'/0'/0/0",
  },
  {
    chain: 'solana',
    address: '11111111111111111111111111111111',
    derivationPath: "m/44'/501'/0'/0'",
  },
  {
    chain: 'tron',
    address: 'T0000000000000000000000000000000001',
    derivationPath: "m/44'/195'/0'/0/0",
  },
];

/** Call Alice's (the app's) core over JSON-RPC and unwrap the {logs,result}. */
async function aliceRpc<T = any>(method: string, params: Record<string, unknown> = {}): Promise<T> {
  const res = await fetch(CORE_RPC_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${CORE_RPC_TOKEN}` },
    body: JSON.stringify({ jsonrpc: '2.0', id: Date.now(), method, params }),
  });
  const body = await res.json();
  if (body.error) throw new Error(`${method} -> ${JSON.stringify(body.error)}`);
  let result = body.result;
  if (result && typeof result === 'object' && 'result' in result && 'logs' in result) {
    result = result.result;
  }
  return result as T;
}

// Wraps aliceRpc as a `{ rpc }` handle so it can be passed to the shared
// `receiveMessage` helper the same way a `launchAgent()` handle is.
const aliceCore = { rpc: aliceRpc };

let bob: Awaited<ReturnType<typeof launchAgent>>;
let aliceCryptoId: string;

test.describe('tiny.place direct messaging (core RPC)', () => {
  test.skip(
    !HAS_TINYPLACE_BACKEND,
    'requires TINYPLACE_API_BASE_URL from the dedicated tiny.place E2E runner'
  );
  test.describe.configure({ mode: 'serial' });

  test.beforeAll(async () => {
    // 1) Give the app's core a fresh tiny.place identity + published Signal keys.
    const mnemonic = await freshMnemonic();
    const encryptedMnemonic = await aliceRpc<string>('openhuman.encrypt_secret', {
      plaintext: mnemonic,
    });
    await aliceRpc('openhuman.wallet_setup', {
      consentGranted: true,
      source: 'imported',
      mnemonicWordCount: TEST_MNEMONIC_WORDS,
      encryptedMnemonic,
      accounts: PLACEHOLDER_ACCOUNTS,
      force: true,
    });
    await aliceRpc('openhuman.tinyplace_signal_provision', { preKeyCount: 10 });
    await aliceRpc('openhuman.tinyplace_signal_register_encryption_key', {});
    const status = await aliceRpc<{ agentId: string }>('openhuman.tinyplace_signal_key_status', {});
    aliceCryptoId = status.agentId;
    expect(aliceCryptoId, 'app core produced a cryptoId').toBeTruthy();

    // 2) Launch the peer core (Bob) and make Alice + Bob accepted contacts so
    //    the relay will carry their DMs.
    bob = await launchAgent('pw-bob', { port: 17851, backend: BACKEND });
    await aliceRpc('openhuman.tinyplace_contacts_request', { agentId: bob.cryptoId });
    await bob.rpc('openhuman.tinyplace_contacts_accept', { agentId: aliceCryptoId });
  });

  test.afterAll(() => {
    bob?.stop();
  });

  test('Alice sends an encrypted DM the peer decrypts, and decrypts the peer reply', async () => {
    // Send an end-to-end encrypted message from the app's own core.
    const outgoing = `alice → bob @ ${Date.now()}`;
    const sent = await aliceRpc<{ encrypted: boolean; messageId: string }>(
      'openhuman.tinyplace_signal_send_message',
      { recipient: bob.cryptoId, plaintext: outgoing }
    );
    expect(sent.encrypted, 'send reports the message was encrypted').toBe(true);

    // The real peer core receives + decrypts exactly what the app's core sent.
    const received = await receiveMessage(bob, { fromCryptoId: aliceCryptoId, timeoutMs: 12_000 });
    expect(received, 'peer decrypts the message sent from the app core').toBe(outgoing);

    // Now the peer replies; the app's own core must receive + decrypt it.
    const reply = `bob → alice @ ${Date.now()}`;
    await bob.rpc('openhuman.tinyplace_signal_send_message', {
      recipient: aliceCryptoId,
      plaintext: reply,
    });

    const decrypted = await receiveMessage(aliceCore, {
      fromCryptoId: bob.cryptoId,
      timeoutMs: 15_000,
    });
    expect(decrypted, 'app core decrypts the peer reply').toBe(reply);
  });
});
