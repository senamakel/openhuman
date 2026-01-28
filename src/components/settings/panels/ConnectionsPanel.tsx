import { useState, useMemo } from 'react';
import { useAppSelector } from '../../../store/hooks';
import { selectIsAuthenticated } from '../../../store/telegramSelectors';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import SettingsPanelLayout from '../components/SettingsPanelLayout';
import TelegramConnectionModal from '../../TelegramConnectionModal';

import BinanceIcon from '../../../assets/icons/binance.svg';
import NotionIcon from '../../../assets/icons/notion.svg';
import TelegramIcon from '../../../assets/icons/telegram.svg';
import MetamaskIcon from '../../../assets/icons/metamask.svg';
import GoogleIcon from '../../../assets/icons/GoogleIcon';

interface ConnectOption {
  id: string;
  name: string;
  description: string;
  icon: React.ReactElement;
  comingSoon?: boolean;
}

// Reused from ConnectStep.tsx - helper to check saved session
const hasSavedSession = (): boolean => {
  try {
    return !!localStorage.getItem('telegram_session');
  } catch {
    return false;
  }
};

const ConnectionsPanel = () => {
  const { navigateBack } = useSettingsNavigation();
  const [isTelegramModalOpen, setIsTelegramModalOpen] = useState(false);

  // Redux state
  const isTelegramAuthenticated = useAppSelector(selectIsAuthenticated);
  const sessionString = useAppSelector((state) => state.telegram.sessionString);

  // Check if Telegram account is connected (authenticated or has saved session)
  const isTelegramConnected = useMemo(() => {
    return isTelegramAuthenticated || !!sessionString || hasSavedSession();
  }, [isTelegramAuthenticated, sessionString]);

  // Connection options - reused from ConnectStep.tsx
  const connectOptions: ConnectOption[] = [
    {
      id: 'telegram',
      name: 'Telegram',
      description: 'Organize chats, automate messages and get insights.',
      icon: <img src={TelegramIcon} alt="Telegram" className="w-5 h-5" />,
    },
    {
      id: 'google',
      name: 'Google',
      description: 'Manage emails, contacts and calendar events',
      icon: <GoogleIcon />,
      comingSoon: true,
    },
    {
      id: 'notion',
      name: 'Notion',
      description: 'Manage tasks, documents and everything else in your Notion',
      icon: <img src={NotionIcon} alt="Notion" className="w-5 h-5" />,
      comingSoon: true,
    },
    {
      id: 'wallet',
      name: 'Web3 Wallet',
      description: 'Trade the trenches in a safe and secure way.',
      icon: <img src={MetamaskIcon} alt="Metamask" className="w-5 h-5" />,
      comingSoon: true,
    },
    {
      id: 'exchange',
      name: 'Crypto Trading Exchanges',
      description: 'Connect and make trades with deep insights.',
      icon: <img src={BinanceIcon} alt="Binance" className="w-5 h-5" />,
      comingSoon: true,
    },
  ];

  // Check if an account is connected
  const isAccountConnected = (accountId: string): boolean => {
    if (accountId === 'telegram') {
      return isTelegramConnected;
    }
    // Add other account checks here when implemented
    return false;
  };

  const handleConnect = (provider: string) => {
    if (provider === 'telegram') {
      if (isTelegramConnected) {
        // TODO: Show disconnect confirmation
        console.log('Disconnect Telegram');
      } else {
        setIsTelegramModalOpen(true);
      }
      return;
    }

    if (connectOptions.find(opt => opt.id === provider)?.comingSoon) {
      console.log(`${provider} coming soon`);
      return;
    }

    console.log(`Connecting to ${provider}`);
  };

  const handleTelegramComplete = () => {
    setIsTelegramModalOpen(false);
  };

  const getConnectionStatus = (optionId: string) => {
    const isConnected = isAccountConnected(optionId);
    if (isConnected) {
      return { label: 'Connected', className: 'bg-green-500/20 text-green-400 border-green-500/30' };
    }
    const option = connectOptions.find(opt => opt.id === optionId);
    if (option?.comingSoon) {
      return { label: 'Coming Soon', className: 'bg-stone-500/20 text-stone-400 border-stone-500/30' };
    }
    return { label: 'Not Connected', className: 'bg-red-500/20 text-red-400 border-red-500/30' };
  };

  return (
    <>
      <SettingsPanelLayout title="Connections" onBack={navigateBack}>
        <div className="p-6">
          <div className="mb-6">
            <h3 className="text-lg font-semibold text-white mb-2">Connected Services</h3>
            <p className="text-sm text-stone-400">
              Manage your connected accounts and services. Connect more accounts for better AI intelligence.
            </p>
          </div>

          <div className="space-y-3">
            {connectOptions.map((option) => {
              const isConnected = isAccountConnected(option.id);
              const status = getConnectionStatus(option.id);

              return (
                <div
                  key={option.id}
                  className="relative flex items-center justify-between p-3 bg-black/50 border border-stone-700 rounded-xl hover:bg-stone-800/30 transition-colors"
                >
                  {/* Connection status dot - top right corner */}
                  {isConnected && !option.comingSoon && (
                    <div className="absolute -top-1 -right-1 w-3 h-3 bg-sage-500 rounded-full border-2 border-black/50"></div>
                  )}

                  <div className="flex items-center space-x-3 flex-1 min-w-0">
                    <div className="flex-shrink-0">
                      {option.icon}
                    </div>
                    <div className="flex-1 min-w-0">
                      <h4 className="font-medium text-white text-sm">{option.name}</h4>
                      <p className="text-xs text-stone-400 truncate">{option.description}</p>
                    </div>
                  </div>

                  <div className="flex items-center space-x-3">
                    {/* Coming Soon badge in original position */}
                    {option.comingSoon && (
                      <span className="px-2 py-1 text-xs font-medium rounded-full border bg-stone-500/20 text-stone-400 border-stone-500/30">
                        Coming Soon
                      </span>
                    )}

                    {/* Action button */}
                    {!option.comingSoon && (
                      <button
                        onClick={() => handleConnect(option.id)}
                        className={`px-3 py-1.5 text-xs font-medium rounded-lg transition-colors ${
                          isConnected
                            ? 'bg-red-500/20 text-red-400 hover:bg-red-500/30 border border-red-500/30'
                            : 'bg-blue-500/20 text-blue-400 hover:bg-blue-500/30 border border-blue-500/30'
                        }`}
                      >
                        {isConnected ? 'Disconnect' : 'Connect'}
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
          </div>

          {/* Security notice */}
          <div className="mt-6 p-4 bg-blue-500/10 border border-blue-500/20 rounded-xl">
            <div className="flex items-start space-x-2">
              <svg className="w-5 h-5 text-blue-400 flex-shrink-0 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              <div>
                <p className="font-medium text-blue-300 text-sm">🔒 Privacy & Security</p>
                <p className="text-blue-200 text-xs mt-1">
                  All data and credentials are stored locally with zero-data retention policy.
                  Your information is encrypted and never shared with third parties.
                </p>
              </div>
            </div>
          </div>
        </div>
      </SettingsPanelLayout>

      {/* Telegram Connection Modal */}
      <TelegramConnectionModal
        isOpen={isTelegramModalOpen}
        onClose={() => setIsTelegramModalOpen(false)}
        onComplete={handleTelegramComplete}
      />
    </>
  );
};

export default ConnectionsPanel;