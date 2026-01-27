import { useState } from 'react';
import GoogleIcon from './icons/GoogleIcon';
import TelegramConnectionModal from '../../../components/TelegramConnectionModal';

import BinanceIcon from './icons/binance.svg';
import NotionIcon from './icons/notion.svg';
import TelegramIcon from './icons/telegram.svg';
import MetamaskIcon from './icons/metamask.svg';


interface ConnectStepProps {
  onNext: () => void;
}

interface ConnectOption {
  id: string;
  name: string;
  description: string;
  icon: React.ReactElement;
  comingSoon?: boolean;
}

const ConnectStep = ({ onNext }: ConnectStepProps) => {
  const [isTelegramModalOpen, setIsTelegramModalOpen] = useState(false);

  const handleConnect = (provider: string) => {
    // In a real app, this would handle OAuth
    console.log(`Connecting to ${provider}`);

    if (provider === 'telegram') {
      setIsTelegramModalOpen(true);
      return;
    }

    // Don't auto-advance for coming soon items
    if (!connectOptions.find(opt => opt.id === provider)?.comingSoon) {
      onNext();
    }
  };

  const handleTelegramComplete = () => {
    setIsTelegramModalOpen(false);
    onNext();
  };

  const connectOptions: ConnectOption[] = [
    {
      id: 'google',
      name: 'Google',
      description: 'Manage emails, contacts and calendar events',
      icon: <GoogleIcon />,
    },
    {
      id: 'notion',
      name: 'Notion',
      description: 'Manage tasks, documents and everything else in your Notion',
      icon: <img src={NotionIcon} alt="Notion" className="w-5 h-5" />,
    },
    {
      id: 'telegram',
      name: 'Telegram',
      description: 'Organize chats, automate messages and get insights.',
      icon: <img src={TelegramIcon} alt="Telegram" className="w-5 h-5" />,
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
      description: 'Connect tand make trades with deep insights.',
      icon: <img src={BinanceIcon} alt="Binance" className="w-5 h-5" />,
      comingSoon: true,
    },
  ];

  return (
    <div className="glass rounded-3xl p-8 shadow-large animate-fade-up">
      <div className="text-center mb-4">
        <h1 className="text-xl font-bold mb-2">Connect Accounts</h1>
        <p className="opacity-70 text-sm">
          The more accounts you connect, the more powerful the intelligence will be.
        </p>
      </div>

      <div className="space-y-3 mb-4">
        {connectOptions.map((option) => (
          <button
            key={option.id}
            onClick={() => handleConnect(option.id)}
            disabled={option.comingSoon}
            className={`w-full flex items-start space-x-3 p-3 bg-black/50 border border-stone-700 rounded-xl hover:border-stone-600 hover:shadow-medium transition-all duration-200 text-left ${option.comingSoon ? 'opacity-50 cursor-not-allowed' : ''
              }`}
          >
            <div className="flex-shrink-0 mt-0.5">{option.icon}</div>
            <div className="flex-1 min-w-0">
              <div className="flex items-center justify-between">
                <span className="font-medium text-sm">{option.name}</span>
                {option.comingSoon && (
                  <span className="text-xs opacity-60 bg-stone-700 px-2 py-0.5 rounded">Coming Soon</span>
                )}
              </div>
              <p className="opacity-70 text-xs mt-1">{option.description}</p>
            </div>
          </button>
        ))}
      </div>

      <div className="mt-4 p-4 bg-sage-500/10 rounded-xl border border-sage-500/30">
        <div className="flex items-start space-x-2">
          <div>
            <p className="font-medium text-sm">🔒 Remember everything is private &amp; encrypted!</p>
            <p className="opacity-70 text-xs mt-1">All data and credentials are stored
              locally and follows a strict zero-data retention policy so you won't have to worry about anything
              getting leaked.</p>
          </div>
        </div>
      </div>

      <TelegramConnectionModal
        isOpen={isTelegramModalOpen}
        onClose={() => setIsTelegramModalOpen(false)}
        onComplete={handleTelegramComplete}
      />
    </div>
  );
};

export default ConnectStep;
