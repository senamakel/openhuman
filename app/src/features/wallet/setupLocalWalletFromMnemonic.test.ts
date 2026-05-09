import { describe, expect, it, vi } from 'vitest';

import { persistLocalWalletFromMnemonic } from './setupLocalWalletFromMnemonic';

const mockSetupLocalWallet = vi.fn();

vi.mock('../../services/walletApi', () => ({
  setupLocalWallet: (...args: unknown[]) => mockSetupLocalWallet(...args),
}));

describe('persistLocalWalletFromMnemonic', () => {
  it('derives multi-chain wallet metadata and persists it after storing the AES key', async () => {
    const setEncryptionKey = vi.fn(async () => undefined);
    mockSetupLocalWallet.mockResolvedValueOnce({
      configured: true,
      onboardingCompleted: true,
      consentGranted: true,
      source: 'generated',
      mnemonicWordCount: 12,
      accounts: [],
      updatedAtMs: 123,
    });

    const mnemonic =
      'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';

    await persistLocalWalletFromMnemonic({ mnemonic, source: 'generated', setEncryptionKey });

    expect(setEncryptionKey).toHaveBeenCalledWith(
      'dce707ee483afb0a70cb2e076295f9f914e0c62cc097895eabda1c0c1f2f0cb1'
    );
    expect(mockSetupLocalWallet).toHaveBeenCalledWith({
      consentGranted: true,
      source: 'generated',
      mnemonicWordCount: 12,
      accounts: [
        {
          chain: 'evm',
          address: '0x9858EfFD232B4033E47d90003D41EC34EcaEda94',
          derivationPath: "m/44'/60'/0'/0/0",
        },
        {
          chain: 'btc',
          address: '1LqBGSKuX5yYUonjxT5qGfpUsXKYYWeabA',
          derivationPath: "m/44'/0'/0'/0/0",
        },
        {
          chain: 'solana',
          address: 'HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk',
          derivationPath: "m/44'/501'/0'/0'",
        },
        {
          chain: 'tron',
          address: 'TUEZSdKsoDHQMeZwihtdoBiN46zxhGWYdH',
          derivationPath: "m/44'/195'/0'/0/0",
        },
      ],
    });
  });

  it('rejects whitespace-only input without touching encryption key or wallet store', async () => {
    const setEncryptionKey = vi.fn(async () => undefined);
    mockSetupLocalWallet.mockReset();

    await expect(
      persistLocalWalletFromMnemonic({ mnemonic: '   \t  ', source: 'imported', setEncryptionKey })
    ).rejects.toThrow(/recovery phrase is required/i);
    expect(setEncryptionKey).not.toHaveBeenCalled();
    expect(mockSetupLocalWallet).not.toHaveBeenCalled();
  });
});
