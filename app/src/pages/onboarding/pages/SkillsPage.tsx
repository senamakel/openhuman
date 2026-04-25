import { useNavigate } from 'react-router-dom';

import { useOnboardingContext } from '../OnboardingContext';
import SkillsStep, { type SkillsConnections } from '../steps/SkillsStep';

const SkillsPage = () => {
  const navigate = useNavigate();
  const { setDraft, completeAndExit } = useOnboardingContext();

  const handleNext = async ({ sources, gmailAccountId }: SkillsConnections) => {
    console.debug('[onboarding:skills-page] next', { sources, gmailAccountId });
    setDraft(prev => ({ ...prev, connectedSources: sources, gmailAccountId }));

    // Route to ContextGatheringStep when there's a gmail source the
    // pipeline can drive — webview gmail (via CDP) or composio gmail
    // (via API). Otherwise jump straight to onboarding completion.
    const hasGmailWebview = sources.includes('webview:gmail') && !!gmailAccountId;
    const hasComposioSource = sources.some(s => s.startsWith('composio:'));
    if (hasGmailWebview || hasComposioSource) {
      navigate('/onboarding/context');
    } else {
      await completeAndExit();
    }
  };

  return <SkillsStep onNext={handleNext} onBack={() => navigate('/onboarding/welcome')} />;
};

export default SkillsPage;
