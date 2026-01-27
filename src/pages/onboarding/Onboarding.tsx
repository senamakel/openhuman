import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import ProgressIndicator from '../../components/ProgressIndicator';
import PrivacyStep from './steps/PrivacyStep';
import AnalyticsStep from './steps/AnalyticsStep';
import ConnectStep from './steps/ConnectStep';
import GetStartedStep from './steps/GetStartedStep';

const Onboarding = () => {
  const navigate = useNavigate();
  const [currentStep, setCurrentStep] = useState(1);
  const totalSteps = 4;

  const handleNext = () => {
    if (currentStep < totalSteps) {
      setCurrentStep(currentStep + 1);
    }
  };

  const handleBack = () => {
    if (currentStep > 1) {
      setCurrentStep(currentStep - 1);
    } else {
      navigate('/');
    }
  };

  const handleComplete = () => {
    navigate('/home');
  };

  const renderStep = () => {
    switch (currentStep) {
      case 1:
        return <PrivacyStep onNext={handleNext} />;
      case 2:
        return <AnalyticsStep onNext={handleNext} />;
      case 3:
        return <ConnectStep onNext={handleNext} />;
      case 4:
        return <GetStartedStep onComplete={handleComplete} />;
      default:
        return <PrivacyStep onNext={handleNext} />;
    }
  };

  return (
    <div className="min-h-screen relative flex items-center justify-center">
      <div className="relative z-10 max-w-lg w-full mx-4">
        <ProgressIndicator currentStep={currentStep} totalSteps={totalSteps} />
        {renderStep()}
        {currentStep > 1 && (
          <button
            onClick={handleBack}
            className="mt-6 w-full opacity-60 hover:opacity-100 text-sm font-medium transition-opacity"
          >
            ← Back
          </button>
        )}
      </div>
    </div>
  );
};

export default Onboarding;
