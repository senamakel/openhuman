import { useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { useT } from '../../../lib/i18n/I18nContext';
import { trackEvent } from '../../../services/analytics';
import ConfigureLaterCallout from '../components/ConfigureLaterCallout';
import {
  CUSTOM_WIZARD_ROUTES,
  CUSTOM_WIZARD_SETTINGS_ROUTES,
  CUSTOM_WIZARD_STEPS,
} from '../customWizardSteps';
import type { CustomStepChoice } from '../OnboardingContext';
import { useOnboardingContext } from '../OnboardingContext';
import CustomWizardStep from '../steps/CustomWizardStep';

const STEP_KEY = 'voice' as const;
const STEP_INDEX = CUSTOM_WIZARD_STEPS.indexOf(STEP_KEY);

const CustomVoicePage = () => {
  const { t } = useT();
  const navigate = useNavigate();
  const { draft, setDraft } = useOnboardingContext();

  const [choice, setChoice] = useState<CustomStepChoice | null>(
    draft.customChoices?.[STEP_KEY] ?? null
  );

  const persistChoice = (next: CustomStepChoice) => {
    setChoice(next);
    setDraft(prev => ({
      ...prev,
      customChoices: { ...prev.customChoices, [STEP_KEY]: next },
    }));
  };

  return (
    <CustomWizardStep
      testId="onboarding-custom-voice-step"
      stepIndex={STEP_INDEX}
      stepCount={CUSTOM_WIZARD_STEPS.length}
      title={t('onboarding.custom.voice.title')}
      subtitle={t('onboarding.custom.voice.subtitle')}
      defaultDescription={t('onboarding.custom.voice.defaultDesc')}
      configureDescription={t('onboarding.custom.voice.configureDesc')}
      configureContent={<ConfigureLaterCallout settingsHref={CUSTOM_WIZARD_SETTINGS_ROUTES[STEP_KEY]} />}
      choice={choice}
      onChoiceChange={persistChoice}
      onBack={() => navigate(CUSTOM_WIZARD_ROUTES[CUSTOM_WIZARD_STEPS[STEP_INDEX - 1]])}
      onContinue={() => {
        trackEvent('onboarding_step_complete', {
          step_name: 'custom_voice',
          choice: choice ?? 'default',
        });
        navigate(CUSTOM_WIZARD_ROUTES[CUSTOM_WIZARD_STEPS[STEP_INDEX + 1]]);
      }}
    />
  );
};

export default CustomVoicePage;
