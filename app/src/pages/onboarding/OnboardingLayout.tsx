import { useCallback, useMemo, useState } from 'react';
import { Outlet, useNavigate } from 'react-router-dom';

import { useCoreState } from '../../providers/CoreStateProvider';
import { userApi } from '../../services/api/userApi';
import { purgeWebviewAccount } from '../../services/webviewAccountService';
import { removeAccount } from '../../store/accountsSlice';
import { useAppDispatch } from '../../store/hooks';
import { getDefaultEnabledTools } from '../../utils/toolDefinitions';
import BetaBanner from './components/BetaBanner';
import { OnboardingContext, type OnboardingDraft } from './OnboardingContext';

/**
 * Full-page chrome for the onboarding flow. Hosts the shared draft + the
 * completion side-effects (persist `onboarding_completed`, notify backend,
 * navigate to /home). Individual steps render through `<Outlet />`.
 */
const OnboardingLayout = () => {
  const navigate = useNavigate();
  const dispatch = useAppDispatch();
  const { setOnboardingCompletedFlag, setOnboardingTasks, snapshot } = useCoreState();
  const [draft, setDraftState] = useState<OnboardingDraft>({ connectedSources: [] });

  const setDraft = useCallback(
    (updater: (prev: OnboardingDraft) => OnboardingDraft) => setDraftState(updater),
    []
  );

  const completeAndExit = useCallback(async () => {
    console.debug('[onboarding:layout] completeAndExit', {
      connectedSources: draft.connectedSources,
      gmailAccountId: draft.gmailAccountId,
    });

    // Tear down the kept-alive gmail webview, if any. SkillsStep opted
    // into `keepAliveOnConnected` so ContextGatheringStep could drive
    // its CDP session — we own the cleanup at the end of the flow.
    if (draft.gmailAccountId) {
      try {
        await purgeWebviewAccount(draft.gmailAccountId);
      } catch (e) {
        console.warn('[onboarding:layout] failed to purge gmail webview', e);
      }
      dispatch(removeAccount({ accountId: draft.gmailAccountId }));
    }

    await setOnboardingTasks({
      accessibilityPermissionGranted:
        snapshot.localState.onboardingTasks?.accessibilityPermissionGranted ?? false,
      localModelConsentGiven: false,
      localModelDownloadStarted: false,
      enabledTools: getDefaultEnabledTools(),
      connectedSources: draft.connectedSources,
      updatedAtMs: Date.now(),
    });

    try {
      await userApi.onboardingComplete();
    } catch {
      console.warn('[onboarding] Failed to notify backend of onboarding completion');
    }

    try {
      await setOnboardingCompletedFlag(true);
    } catch (e) {
      console.error('[onboarding] Failed to persist onboarding_completed', e);
      throw e;
    }

    navigate('/home', { replace: true });
  }, [
    draft.connectedSources,
    draft.gmailAccountId,
    dispatch,
    navigate,
    setOnboardingCompletedFlag,
    setOnboardingTasks,
    snapshot,
  ]);

  const value = useMemo(
    () => ({ draft, setDraft, completeAndExit }),
    [draft, setDraft, completeAndExit]
  );

  return (
    <OnboardingContext.Provider value={value}>
      <div
        data-testid="onboarding-layout"
        className="min-h-full relative flex items-center justify-center py-10">
        <div className="relative z-10 w-full max-w-lg mx-4">
          <BetaBanner />
          <Outlet />
        </div>
      </div>
    </OnboardingContext.Provider>
  );
};

export default OnboardingLayout;
