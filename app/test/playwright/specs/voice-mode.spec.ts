import { expect, test } from '@playwright/test';

import { bootAuthenticatedPage, callCoreRpc } from '../helpers/core-rpc';

test.describe('Voice mode integration', () => {
  test.skip(
    'chat voice toggle UI was removed; migrate against the mascot voice path instead',
    async () => {}
  );
});

test.describe('Voice mode - offline STT contract (voice_status RPC)', () => {
  test.beforeEach(async ({ page }) => {
    await bootAuthenticatedPage(page, 'pw-voice-mode', '/home');
  });

  test('voice_status RPC returns a well-formed response', async () => {
    const status = await callCoreRpc<unknown>('openhuman.voice_status', {});
    const root = (status ?? {}) as Record<string, unknown>;
    const payload =
      root && typeof root === 'object' && 'result' in root
        ? (root.result as Record<string, unknown>)
        : root;

    expect(typeof payload.stt_available).toBe('boolean');
    expect(typeof payload.tts_available).toBe('boolean');
    expect(typeof payload.stt_provider).toBe('string');
  });

  test('voice_status reports a declared provider even when local assets are unavailable', async () => {
    const status = await callCoreRpc<unknown>('openhuman.voice_status', {});
    const root = (status ?? {}) as Record<string, unknown>;
    const payload =
      root && typeof root === 'object' && 'result' in root
        ? (root.result as Record<string, unknown>)
        : root;

    const sttProvider = String(payload.stt_provider ?? '');
    expect(sttProvider.length).toBeGreaterThan(0);

    const whisperBinary = payload.whisper_binary;
    const sttModelPath = payload.stt_model_path;
    if ((sttProvider === 'whisper' || sttProvider === 'local') && !whisperBinary && !sttModelPath) {
      expect(payload.stt_available).toBe(false);
    }
  });
});
