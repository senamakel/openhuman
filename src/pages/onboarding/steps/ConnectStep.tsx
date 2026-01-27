interface ConnectStepProps {
  onNext: () => void;
}

const ConnectStep = ({ onNext }: ConnectStepProps) => {
  const handleConnect = (provider: string) => {
    // In a real app, this would handle OAuth
    console.log(`Connecting to ${provider}`);
    onNext();
  };

  return (
    <div className="glass rounded-3xl p-8 shadow-large animate-fade-up">
      <div className="text-center mb-4">
        <h1 className="text-xl font-bold mb-2">Connect Accounts</h1>
        <p className="opacity-70 text-sm">
          Connect your accounts to personalize your experience
        </p>
      </div>

      <div className="space-y-3 mb-4">
        <button
          onClick={() => handleConnect('google')}
          className="w-full flex items-center justify-center space-x-3 p-3 bg-black/50 border border-stone-700 rounded-xl hover:border-stone-600 hover:shadow-medium transition-all duration-200"
        >
          <svg className="w-5 h-5" viewBox="0 0 24 24">
            <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"/>
            <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"/>
            <path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"/>
            <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"/>
          </svg>
          <span className="font-medium text-sm">Use Google</span>
        </button>

        <button
          onClick={() => handleConnect('microsoft')}
          className="w-full flex items-center justify-center space-x-3 p-3 bg-black/50 border border-stone-700 rounded-xl hover:border-stone-600 hover:shadow-medium transition-all duration-200"
        >
          <svg className="w-5 h-5" viewBox="0 0 24 24">
            <path fill="#f25022" d="M1 1h10v10H1z"/>
            <path fill="#00a4ef" d="M13 1h10v10H13z"/>
            <path fill="#7fba00" d="M1 13h10v10H1z"/>
            <path fill="#ffb900" d="M13 13h10v10H13z"/>
          </svg>
          <span className="font-medium text-sm">Use Microsoft</span>
        </button>

        <button
          onClick={() => handleConnect('discord')}
          className="w-full flex items-center justify-center space-x-3 p-3 bg-black/50 border border-stone-700 rounded-xl hover:border-stone-600 hover:shadow-medium transition-all duration-200"
        >
          <svg className="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
            <path d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028c.462-.63.874-1.295 1.226-1.994a.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03zM8.02 15.33c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.956-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.956 2.418-2.157 2.418zm7.975 0c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.955-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.946 2.418-2.157 2.418z"/>
          </svg>
          <span className="font-medium text-sm">Use Discord</span>
        </button>

        <button
          onClick={() => handleConnect('twitter')}
          className="w-full flex items-center justify-center space-x-3 p-3 bg-black/50 border border-stone-700 rounded-xl hover:border-stone-600 hover:shadow-medium transition-all duration-200"
        >
          <svg className="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
            <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/>
          </svg>
          <span className="font-medium text-sm">Use Twitter/X</span>
        </button>
      </div>

      <button
        onClick={onNext}
        className="w-full py-2.5 opacity-60 hover:opacity-100 font-medium text-sm transition-opacity"
      >
        Skip for now
      </button>

      <div className="mt-4 p-4 bg-stone-800/50 rounded-xl border border-stone-700">
        <div className="flex items-start space-x-2">
          <svg className="w-5 h-5 text-primary-400 mt-0.5" fill="currentColor" viewBox="0 0 20 20">
            <path fillRule="evenodd" d="M10 1L5 6v4c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V6l-5-5z"/>
          </svg>
          <div>
            <p className="font-medium text-sm">Your data stays private</p>
            <p className="opacity-70 text-xs mt-1">We only use connected accounts for account notifications and security</p>
          </div>
        </div>
      </div>
    </div>
  );
};

export default ConnectStep;
