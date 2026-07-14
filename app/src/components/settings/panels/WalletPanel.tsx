import { useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import ChipTabs from '../../layout/ChipTabs';
import RecoveryPhrasePanel from './RecoveryPhrasePanel';
import WalletBalancesPanel from './WalletBalancesPanel';

type WalletTab = 'balance' | 'recovery';

/**
 * WalletPanel — the Connections "Wallet" destination as a two-tab view:
 * **Wallet balance** (multi-chain balances) and **Recovery** (recovery phrase).
 * A chip row switches between the two existing panels, which each keep their own
 * header + scroll.
 */
export default function WalletPanel() {
  const { t } = useT();
  const [tab, setTab] = useState<WalletTab>('balance');

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="wallet-panel">
      <div className="flex-shrink-0 border-b border-line bg-surface-muted px-4 py-2">
        <ChipTabs<WalletTab>
          as="tab"
          ariaLabel={t('wallet.ariaLabel')}
          testIdPrefix="wallet"
          className="inline-flex flex-wrap items-center gap-1.5"
          items={[
            { id: 'balance', label: t('wallet.tabs.balance') },
            { id: 'recovery', label: t('wallet.tabs.recovery') },
          ]}
          value={tab}
          onChange={setTab}
        />
      </div>
      <div className="min-h-0 flex-1">
        {tab === 'balance' ? <WalletBalancesPanel /> : <RecoveryPhrasePanel />}
      </div>
    </div>
  );
}
