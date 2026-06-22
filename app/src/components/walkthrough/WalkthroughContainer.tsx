import { useOnboardingContext } from '../../pages/onboarding/OnboardingContext';
import WalkthroughPhasePanel from './WalkthroughPhasePanel';
import { WalkthroughProvider } from './WalkthroughProvider';

/**
 * Top-level walkthrough container. Reads walkthrough state from the
 * OnboardingContext and wires it into the WalkthroughProvider + PhasePanel.
 *
 * Mount this inside the onboarding layout (within OnboardingContext.Provider).
 */
const WalkthroughContainer = () => {
  const { draft, advanceWalkthrough, skipWalkthrough } = useOnboardingContext();

  const walkthrough = draft.walkthrough;
  if (!walkthrough) {
    return null;
  }

  return (
    <WalkthroughProvider
      state={walkthrough}
      onAdvance={advanceWalkthrough}
      onSkip={skipWalkthrough}>
      <WalkthroughPhasePanel />
    </WalkthroughProvider>
  );
};

export default WalkthroughContainer;
