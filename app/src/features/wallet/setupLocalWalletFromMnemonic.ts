import { setupLocalWallet } from '../../services/walletApi';
import {
  deriveAesKeyFromMnemonic,
  deriveWalletAccountsFromMnemonic,
  type WalletSetupSource,
} from '../../utils/cryptoKeys';

export async function persistLocalWalletFromMnemonic(args: {
  mnemonic: string;
  source: WalletSetupSource;
  setEncryptionKey: (value: string | null) => Promise<void>;
}): Promise<void> {
  const { mnemonic, source, setEncryptionKey } = args;
  const trimmedMnemonic = mnemonic.trim();
  const aesKey = deriveAesKeyFromMnemonic(trimmedMnemonic);

  await setEncryptionKey(aesKey);
  await setupLocalWallet({
    consentGranted: true,
    source,
    mnemonicWordCount: trimmedMnemonic.split(/\s+/).length,
    accounts: deriveWalletAccountsFromMnemonic(trimmedMnemonic),
  });
}
