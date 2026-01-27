import { useState } from 'react';

interface AnalyticsStepProps {
  onNext: () => void;
}

const AnalyticsStep = ({ onNext }: AnalyticsStepProps) => {
  const [selectedOption, setSelectedOption] = useState('maximumPrivacy');

  return (
    <div className="glass rounded-3xl p-8 shadow-large animate-fade-up">
      <div className="text-center mb-4">
        <h1 className="text-xl font-bold mb-2">Analytics</h1>
        <p className="opacity-70 text-sm">
          Help us improve your experience while maintaining your privacy
        </p>
      </div>

      <div className="space-y-4 mb-4">
        <div
          className={`p-4 rounded-xl border-2 cursor-pointer transition-all ${
            selectedOption === 'shareAnalytics'
              ? 'border-primary-500 bg-primary-500/20'
              : 'border-stone-700 bg-black/50 hover:border-stone-600'
          }`}
          onClick={() => setSelectedOption('shareAnalytics')}
        >
          <div className="flex items-start space-x-4">
            <div className="flex items-center justify-center mt-0.5">
              <div
                className={`w-5 h-5 rounded-full border-2 flex items-center justify-center ${
                  selectedOption === 'shareAnalytics'
                    ? 'border-primary-500 bg-primary-500'
                    : 'border-stone-600 bg-black'
                }`}
              >
                {selectedOption === 'shareAnalytics' && (
                  <div className="w-2 h-2 bg-white rounded-full"></div>
                )}
              </div>
            </div>
            <div>
              <h3 className="font-semibold mb-1 text-sm">Securely Share Analytics</h3>
              <p className="opacity-70 text-xs leading-relaxed">
                Share anonymized usage data to help us improve features and performance. All data is encrypted and cannot be traced back to you.
              </p>
            </div>
          </div>
        </div>

        <div
          className={`p-4 rounded-xl border-2 cursor-pointer transition-all ${
            selectedOption === 'maximumPrivacy'
              ? 'border-primary-500 bg-primary-500/20'
              : 'border-stone-700 bg-black/50 hover:border-stone-600'
          }`}
          onClick={() => setSelectedOption('maximumPrivacy')}
        >
          <div className="flex items-start space-x-4">
            <div className="flex items-center justify-center mt-0.5">
              <div
                className={`w-5 h-5 rounded-full border-2 flex items-center justify-center ${
                  selectedOption === 'maximumPrivacy'
                    ? 'border-primary-500 bg-primary-500'
                    : 'border-stone-600 bg-black'
                }`}
              >
                {selectedOption === 'maximumPrivacy' && (
                  <div className="w-2 h-2 bg-white rounded-full"></div>
                )}
              </div>
            </div>
            <div>
              <h3 className="font-semibold mb-1 text-sm">Maximum Privacy</h3>
              <p className="opacity-70 text-xs leading-relaxed">
                Keep all your data completely private. We won't collect any usage analytics, ensuring total anonymity.
              </p>
              <div className="flex items-center space-x-1 mt-2">
                <svg className="w-4 h-4 text-primary-400" fill="currentColor" viewBox="0 0 20 20">
                  <path fillRule="evenodd" d="M10 1L5 6v4c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V6l-5-5z"/>
                </svg>
                <span className="text-primary-400 text-xs font-medium">Recommended for privacy</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <button
        onClick={onNext}
        className="btn-primary w-full py-2.5 text-sm font-medium rounded-xl mb-4"
      >
        Continue
      </button>

      <div className="p-4 bg-stone-800/50 rounded-xl border border-stone-700">
        <div className="flex items-start space-x-2">
          <svg className="w-5 h-5 text-sage-400 mt-0.5" fill="currentColor" viewBox="0 0 20 20">
            <path fillRule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a1 1 0 000 2v3a1 1 0 001 1h1a1 1 0 100-2v-3a1 1 0 00-1-1H9z" clipRule="evenodd"/>
          </svg>
          <div>
            <p className="font-medium text-sm">You can change this setting anytime</p>
            <p className="opacity-70 text-xs mt-1">Your privacy preferences can be updated in your account settings</p>
          </div>
        </div>
      </div>
    </div>
  );
};

export default AnalyticsStep;
