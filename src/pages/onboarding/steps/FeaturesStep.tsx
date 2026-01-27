import PrivacyFeatureCard from '../../../components/PrivacyFeatureCard';

interface FeaturesStepProps {
  onNext: () => void;
}

const FeaturesStep = ({ onNext }: FeaturesStepProps) => {
  const features = [
    {
      title: '🤖 Telegram Bot Assistant',
      description: 'Interact with AlphaHuman through Telegram. Get instant responses, automate tasks, and receive insights directly in your chats.',
    },
    {
      title: '📊 Crypto Market Intelligence',
      description: 'Get real-time market analysis, price alerts, and deep insights to help you make informed trading decisions.',
    },
    {
      title: '🔗 Multi-Account Integration',
      description: 'Connect Google, Notion, Telegram, and more. Your assistant can read emails, manage tasks, and automate workflows across all your tools.',
    },
    {
      title: '⚡ Local Processing',
      description: 'All your data is processed locally on your device. Your conversations, credentials, and sensitive information never leave your machine.',
    },
    {
      title: '🔄 Automation & Workflows',
      description: 'Automate repetitive tasks, schedule actions, and create custom workflows to 10x your productivity in crypto.',
    },
  ];

  return (
    <div className="glass rounded-3xl p-8 shadow-large animate-fade-up">
      <div className="text-center mb-4">
        <h1 className="text-xl font-bold mb-2">Features</h1>
        <p className="opacity-70 text-sm">
          Discover what AlphaHuman can do for you
        </p>
      </div>

      <div className="space-y-2 mb-4">
        {features.map((feature, index) => (
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

export default FeaturesStep;
