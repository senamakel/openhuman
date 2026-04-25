import { createContext, useContext } from 'react';

export interface OnboardingDraft {
  connectedSources: string[];
  /**
   * Account id of the gmail webview the user just signed into, if any.
   * Stays alive (hidden off-screen) through the rest of onboarding so
   * downstream steps (e.g. ContextGatheringStep) can drive the gmail
   * scanner via CDP without reopening the modal.
   */
  gmailAccountId?: string;
}

export interface OnboardingContextValue {
  draft: OnboardingDraft;
  setDraft: (updater: (prev: OnboardingDraft) => OnboardingDraft) => void;
  /**
   * Persist `onboarding_completed=true`, notify the backend (best-effort), and
   * navigate to `/home`. Called by the final step.
   */
  completeAndExit: () => Promise<void>;
}

export const OnboardingContext = createContext<OnboardingContextValue | null>(null);

export function useOnboardingContext(): OnboardingContextValue {
  const ctx = useContext(OnboardingContext);
  if (!ctx) {
    throw new Error('useOnboardingContext must be used within an OnboardingLayout');
  }
  return ctx;
}
