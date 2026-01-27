interface PrivacyFeatureCardProps {
  title: string;
  description: string;
}

const PrivacyFeatureCard = ({ title, description }: PrivacyFeatureCardProps) => {
  return (
    <div className="bg-stone-800/50 rounded-xl p-6 border border-stone-700">
      <div className="flex items-start space-x-4">
        <div>
          <h3 className="font-semibold mb-2">{title}</h3>
          <p className="opacity-70 text-sm leading-relaxed">{description}</p>
        </div>
      </div>
    </div>
  );
};

export default PrivacyFeatureCard;
