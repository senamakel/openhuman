import PrivacyFeatureCard from '../../../components/PrivacyFeatureCard';

interface PrivacyStepProps {
  onNext: () => void;
}

const PrivacyStep = ({ onNext }: PrivacyStepProps) => {
  const privacyFeatures = [
    {
      title: '🔒 Client-Side Encryption',
      description: 'Your data is encrypted in your browser before it ever reaches our servers. We store only ciphertext. Without your recovery phrase, your content is cryptographically unreadable AES-256-GCM. Keys never leave your device',
    },
    {
      title: '🙈 Zero Admin Access',
      description: 'Even with full database access, Momo admins cannot decrypt your content. Your encryption keys exist only in your browser. We have no mechanism to access them.',
    },
    {
      title: '🚫 Zero Data Retention',
      description: 'Your data is NEVER used to train AI models. We operate under a Zero Data Retention contract with Anthropic. Your queries are processed and immediately discarded, never stored or used for training.',
    },
  ];

  return (
    <div className="glass rounded-3xl p-8 shadow-large animate-fade-up">
      <div className="text-center mb-4">
        <h1 className="text-xl font-bold mb-2">Privacy</h1>
        <p className="opacity-70 text-sm">
          A quick overview of how your privacy is protected with AlphaHuman. AlphaHuman is built with privacy in mind.
        </p>
      </div>

      <div className="space-y-2 mb-4">
        {privacyFeatures.map((feature, index) => (
          <PrivacyFeatureCard
            key={index}
            title={feature.title}
            description={feature.description}
          />
        ))}
      </div>

      <button
        onClick={onNext}
        className="btn-primary w-full py-2.5 text-sm font-medium rounded-xl"
      >
        Continue
      </button>
    </div>
  );
};

export default PrivacyStep;
