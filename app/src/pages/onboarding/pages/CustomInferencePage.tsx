import { useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { useT } from '../../../lib/i18n/I18nContext';
import { trackEvent } from '../../../services/analytics';
import { setCloudProviderKey } from '../../../services/api/aiSettingsApi';
import {
  CUSTOM_WIZARD_ROUTES,
  CUSTOM_WIZARD_STEPS,
} from '../customWizardSteps';
import type { CustomStepChoice } from '../OnboardingContext';
import { useOnboardingContext } from '../OnboardingContext';
import CustomWizardStep from '../steps/CustomWizardStep';

const STEP_KEY = 'inference' as const;
const STEP_INDEX = CUSTOM_WIZARD_STEPS.indexOf(STEP_KEY);

const CustomInferencePage = () => {
  const { t } = useT();
  const navigate = useNavigate();
  const { draft, setDraft } = useOnboardingContext();

  const [choice, setChoice] = useState<CustomStepChoice | null>(
    draft.customChoices?.[STEP_KEY] ?? null
  );
  const [openai, setOpenai] = useState('');
  const [anthropic, setAnthropic] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const persistChoice = (next: CustomStepChoice) => {
    setChoice(next);
    setDraft(prev => ({
      ...prev,
      customChoices: { ...prev.customChoices, [STEP_KEY]: next },
    }));
    setError(null);
  };

  const advance = () => {
    trackEvent('onboarding_step_complete', {
      step_name: 'custom_inference',
      choice: choice ?? 'default',
    });
    navigate(CUSTOM_WIZARD_ROUTES[CUSTOM_WIZARD_STEPS[STEP_INDEX + 1]]);
  };

  const handleContinue = async () => {
    if (choice === 'configure') {
      const o = openai.trim();
      const a = anthropic.trim();
      if (!o && !a) {
        // Empty form = treat as configure-later, advance without saving.
        advance();
        return;
      }
      setSaving(true);
      setError(null);
      try {
        if (o) await setCloudProviderKey('openai', o);
        if (a) await setCloudProviderKey('anthropic', a);
        advance();
      } catch (err) {
        console.warn('[onboarding:custom-inference] save failed', err);
        setError(t('onboarding.apiKeys.saveError'));
        setSaving(false);
      }
      return;
    }
    advance();
  };

  return (
    <CustomWizardStep
      testId="onboarding-custom-inference-step"
      stepIndex={STEP_INDEX}
      stepCount={CUSTOM_WIZARD_STEPS.length}
      title={t('onboarding.custom.inference.title')}
      subtitle={t('onboarding.custom.inference.subtitle')}
      defaultDescription={t('onboarding.custom.inference.defaultDesc')}
      configureDescription={t('onboarding.custom.inference.configureDesc')}
      configureContent={
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <label
              htmlFor="onboarding-custom-openai-key"
              className="text-xs font-medium text-stone-700">
              {t('onboarding.apiKeys.openaiLabel')}
            </label>
            <input
              id="onboarding-custom-openai-key"
              data-testid="onboarding-custom-inference-openai-input"
              type="password"
              autoComplete="off"
              spellCheck={false}
              placeholder={t('onboarding.apiKeys.openaiPlaceholder')}
              value={openai}
              onChange={e => {
                setOpenai(e.target.value);
                setError(null);
              }}
              className="rounded-lg border border-stone-300 bg-white px-3 py-2 text-sm text-stone-900 placeholder-stone-400 focus:border-primary-500 focus:outline-none focus:ring-1 focus:ring-primary-500"
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <label
              htmlFor="onboarding-custom-anthropic-key"
              className="text-xs font-medium text-stone-700">
              {t('onboarding.apiKeys.anthropicLabel')}
            </label>
            <input
              id="onboarding-custom-anthropic-key"
              data-testid="onboarding-custom-inference-anthropic-input"
              type="password"
              autoComplete="off"
              spellCheck={false}
              placeholder={t('onboarding.apiKeys.anthropicPlaceholder')}
              value={anthropic}
              onChange={e => {
                setAnthropic(e.target.value);
                setError(null);
              }}
              className="rounded-lg border border-stone-300 bg-white px-3 py-2 text-sm text-stone-900 placeholder-stone-400 focus:border-primary-500 focus:outline-none focus:ring-1 focus:ring-primary-500"
            />
          </div>
          {error ? <p className="text-xs font-medium text-red-600">{error}</p> : null}
        </div>
      }
      choice={choice}
      onChoiceChange={persistChoice}
      onBack={() => navigate('/onboarding/runtime-choice')}
      onContinue={() => void handleContinue()}
      continueLoading={saving}
      continueLoadingLabel={t('onboarding.apiKeys.saving')}
    />
  );
};

export default CustomInferencePage;
