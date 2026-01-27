import { useNavigate } from 'react-router-dom';
import PrivacyFeatureCard from '../../components/PrivacyFeatureCard';

const Step1Privacy = () => {
  const navigate = useNavigate();

  const handleContinue = () => {
    navigate('/onboarding/step2');
  };

  const privacyFeatures = [
    {
      title: 'Client-Side Encryption',
      description: 'Your data is encrypted in your browser before it ever reaches our servers. We store only ciphertext. Without your recovery phrase, your content is cryptographically unreadable AES-256-GCM. Keys never leave your device',
    },
    {
      title: 'Zero Admin Access',
      description: 'Even with full database access, Momo admins cannot decrypt your content. Your encryption keys exist only in your browser. We have no mechanism to access them.',
    },
    {
      title: 'Zero Data Retention',
      description: 'Your data is NEVER used to train AI models. We operate under a Zero Data Retention contract with Anthropic. Your queries are processed and immediately discarded, never stored or used for training.',
    },
  ];

  return (
    <div className="min-h-screen relative flex items-center justify-center">
      {/* Main content */}
      <div className="relative z-10 max-w-md w-full mx-4">
        {/* Progress indicator */}
        <div className="flex items-center justify-center space-x-2 mb-8">
          <div className="flex items-center">
            <div className="w-8 h-8 bg-primary-500 rounded-full flex items-center justify-center text-white text-sm font-semibold">1</div>
            <div className="w-12 h-1 bg-primary-500 mx-2"></div>
          </div>
          <div className="flex items-center">
            <div className="w-8 h-8 bg-stone-700 rounded-full flex items-center justify-center text-white text-sm font-semibold">2</div>
            <div className="w-12 h-1 bg-stone-700 mx-2"></div>
          </div>
          <div className="flex items-center">
            <div className="w-8 h-8 bg-stone-700 rounded-full flex items-center justify-center text-white text-sm font-semibold">3</div>
            <div className="w-12 h-1 bg-stone-700 mx-2"></div>
          </div>
          <div className="w-8 h-8 bg-stone-700 rounded-full flex items-center justify-center text-white text-sm font-semibold">4</div>
        </div>

        {/* Privacy card */}
        <div className="glass rounded-3xl p-8 shadow-large animate-fade-up">
          <div className="text-center mb-8">
            <h1 className="text-xl font-bold mb-2">
              Privacy
            </h1>
            <p className="opacity-70 text-sm">
              A quick overview of how your privacy is protected with AlphaHuman
            </p>
          </div>

          {/* Privacy Features Section */}
          <div className="space-y-4 mb-8">
            {privacyFeatures.map((feature, index) => (
              <PrivacyFeatureCard
                key={index}
                title={feature.title}
                description={feature.description}
              />
            ))}
          </div>

          {/* Continue button */}
          <button
            onClick={handleContinue}
            className="btn-primary w-full py-4 text-lg font-semibold rounded-xl"
          >
            Continue
          </button>
        </div>
      </div>
    </div>
  );
};

export default Step1Privacy;
