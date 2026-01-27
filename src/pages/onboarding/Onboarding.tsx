import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import ProgressIndicator from '../../components/ProgressIndicator';
import LottieAnimation from '../../components/LottieAnimation';
import FeaturesStep from './steps/FeaturesStep';
import PrivacyStep from './steps/PrivacyStep';
import AnalyticsStep from './steps/AnalyticsStep';
import ConnectStep from './steps/ConnectStep';
import GetStartedStep from './steps/GetStartedStep';

const Onboarding = () => {
  const navigate = useNavigate();
  const [currentStep, setCurrentStep] = useState(1);
  const totalSteps = 5;

  // Lottie animation files for each step
  const stepAnimations = [
    '/lottie/wave.json', // Step 1 - Features
    '/lottie/safe3.json', // Step 2 - Privacy
    '/lottie/analytics.json', // Step 3 - Analytics
    '/lottie/connect2.json', // Step 4 - Connect
    '/lottie/trophy.json', // Step 5 - Get Started
  ];

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
        return <FeaturesStep onNext={handleNext} />;
      case 2:
        return <PrivacyStep onNext={handleNext} />;
      case 3:
        return <AnalyticsStep onNext={handleNext} />;
      case 4:
        return <ConnectStep onNext={handleNext} />;
      case 5:
        return <GetStartedStep onComplete={handleComplete} />;
      default:
        return <FeaturesStep onNext={handleNext} />;
    }
  };

  return (
    <div className="min-h-screen relative flex items-center justify-center">
      <div className="relative z-10 max-w-lg w-full mx-4">
        <div className="flex justify-center mb-6">
          <LottieAnimation src={stepAnimations[currentStep - 1]} height={120} width={120} />
        </div>
        <ProgressIndicator currentStep={currentStep} totalSteps={totalSteps} />
        {renderStep()}
        {/* {currentStep > 1 && (
          <button
            onClick={handleBack}
            className="mt-6 outline-none border-none w-full opacity-60 hover:opacity-100 text-sm font-medium transition-opacity"
          >
            ← Back
          </button>
        )} */}
      </div>
    </div>
  );
};

export default Onboarding;
