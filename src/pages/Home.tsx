import { useNavigate } from 'react-router-dom';

import ConnectionIndicator from '../components/ConnectionIndicator';
import ModelDownloadProgress from '../components/ModelDownloadProgress';
import SkillsGrid from '../components/SkillsGrid';
import { useUser } from '../hooks/useUser';
import { TELEGRAM_BOT_USERNAME } from '../utils/config';
import { openUrl } from '../utils/openUrl';

const Home = () => {
  const navigate = useNavigate();
  const { user } = useUser();
  const userName = user?.firstName || 'User';

  // Get greeting based on time
  const getGreeting = () => {
    const hour = new Date().getHours();
    if (hour < 12) return 'Good morning';
    if (hour < 18) return 'Good afternoon';
    return 'Good evening';
  };

  // Handle Telegram bot link
  const handleStartCooking = async () => {
    await openUrl(`https://t.me/${TELEGRAM_BOT_USERNAME}`);
  };

  const handleUpgrade = () => {
    navigate('/settings/billing');
  };

  const currentPlan = user?.subscription?.plan || 'FREE';
  const showUpgradeCTA = currentPlan === 'FREE';

  return (
    <div className="min-h-full relative">
      {/* Content overlay */}
      <div className="relative z-10 min-h-full flex flex-col">
        {/* Main content */}
        <div className="flex-1 flex items-center justify-center p-4">
          <div className="max-w-md w-full">
            {/* Upgrade CTA */}
            {showUpgradeCTA && (
              <button
                onClick={handleUpgrade}
                className="glass rounded-3xl p-4 shadow-large animate-fade-up mb-4 w-full text-left hover:bg-stone-800/30 transition-all duration-200 focus:outline-none focus:ring-2 focus:ring-primary-500/50 group">
                <div className="flex items-center justify-between">
                  <div className="flex-1">
                    <div className="flex items-center gap-2 mb-1">
                      <svg
                        className="w-5 h-5 text-primary-500"
                        fill="none"
                        stroke="currentColor"
                        viewBox="0 0 24 24">
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M13 10V3L4 14h7v7l9-11h-7z"
                        />
                      </svg>
                      <span className="font-semibold text-sm">Upgrade to Premium</span>
                    </div>
                    <p className="text-xs opacity-70">
                      Unlock advanced features and unlimited access
                    </p>
                  </div>
                  <svg
                    className="w-5 h-5 opacity-60 group-hover:opacity-100 transition-opacity"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24">
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M9 5l7 7-7 7"
                    />
                  </svg>
                </div>
              </button>
            )}

            {/* Weather card */}
            <div className="glass rounded-3xl p-4 shadow-large animate-fade-up text-center">
              {/* Greeting */}
              <h1 className="text-2xl font-bold mb-4">
                {getGreeting()}, {userName}
              </h1>

              {/* Connection indicators */}
              <ConnectionIndicator />

              {/* Get Access button */}
              <button
                onClick={handleStartCooking}
                className="btn-primary w-full py-2.5 text-sm font-medium rounded-xl">
                Message AlphaHuman 🔥
              </button>
            </div>

            {/* Skills Grid */}
            <SkillsGrid />

            <ModelDownloadProgress className="mb-4" />
          </div>
        </div>
      </div>
    </div>
  );
};

export default Home;
