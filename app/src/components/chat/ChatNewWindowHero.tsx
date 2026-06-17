import { useEffect, useMemo, useState } from 'react';

import { useUsageState } from '../../hooks/useUsageState';
import { useUser } from '../../hooks/useUser';
import { useT } from '../../lib/i18n/I18nContext';
import { applyOpenRouterFreeModels } from '../../services/api/openrouterFreeModels';
import { restartCoreProcess } from '../../services/coreProcessControl';
import { selectBlockingState } from '../../store/connectivitySelectors';
import { useAppSelector } from '../../store/hooks';
import { resolveUserName } from '../../utils/userName';
import ConnectionIndicator from '../ConnectionIndicator';
import { DiscordBanner, PromotionalCreditsBanner, UsageLimitBanner } from '../home/HomeBanners';

/**
 * Hero shown above the composer in the chat "new window" (empty thread) state —
 * the merged Home surface. Reuses Home's animated greeting and banners, but
 * drops the framing card / version / theme toggle / "Ask Assistant" CTA: the
 * composer directly below is the call to action now. The core-unreachable
 * recovery button is preserved since the composer is disabled while the core is
 * down.
 */
export default function ChatNewWindowHero() {
  const { t } = useT();
  const { user } = useUser();
  const { shouldShowBudgetCompletedMessage } = useUsageState();

  const userName = resolveUserName(user).split(' ')[0];
  const promoCredits = user?.usage?.promotionBalanceUsd ?? 0;
  const isFreeTier =
    user?.subscription?.plan === 'FREE' || !user?.subscription?.hasActiveSubscription;
  const showPromoBanner = isFreeTier && promoCredits > 0.01;

  const blocking = useAppSelector(selectBlockingState);
  const [isRestartingCore, setIsRestartingCore] = useState(false);
  const [restartError, setRestartError] = useState<string | null>(null);
  const [openRouterStatus, setOpenRouterStatus] = useState<'idle' | 'saving' | 'error'>('idle');

  const welcomeVariants = useMemo(
    () => [`Welcome, ${userName} 👋`, `Let's cook, ${userName} 🧑‍🍳.`, `Time to Zone In 🧘🏻`],
    [userName]
  );
  const [welcomeVariantIndex, setWelcomeVariantIndex] = useState(0);
  const [typedWelcome, setTypedWelcome] = useState('');
  const [isDeletingWelcome, setIsDeletingWelcome] = useState(false);

  const statusCopy = {
    ok: t('home.statusOk'),
    'backend-only': t('home.statusBackendOnly'),
    'core-unreachable': t('home.statusCoreUnreachable'),
    'internet-offline': t('home.statusInternetOffline'),
  }[blocking];

  const handleRestartCore = async () => {
    setIsRestartingCore(true);
    setRestartError(null);
    try {
      await restartCoreProcess();
    } catch (err) {
      setRestartError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsRestartingCore(false);
    }
  };

  const handleUseOpenRouterFree = async () => {
    setOpenRouterStatus('saving');
    try {
      await applyOpenRouterFreeModels();
      setOpenRouterStatus('idle');
    } catch (err) {
      console.warn('[chat-hero] applyOpenRouterFreeModels failed', err);
      setOpenRouterStatus('error');
    }
  };

  // Typewriter cycle — identical cadence to the former Home greeting.
  useEffect(() => {
    const activeVariant = welcomeVariants[welcomeVariantIndex] ?? '';
    const isFullyTyped = typedWelcome === activeVariant;
    const isFullyDeleted = typedWelcome.length === 0;

    const delay = isDeletingWelcome
      ? 36
      : isFullyTyped
        ? 1400
        : typedWelcome.length === 0
          ? 250
          : 55;

    const timeoutId = window.setTimeout(() => {
      if (!isDeletingWelcome) {
        if (isFullyTyped) {
          setIsDeletingWelcome(true);
          return;
        }
        setTypedWelcome(activeVariant.slice(0, typedWelcome.length + 1));
        return;
      }
      if (!isFullyDeleted) {
        setTypedWelcome(activeVariant.slice(0, typedWelcome.length - 1));
        return;
      }
      setIsDeletingWelcome(false);
      setWelcomeVariantIndex(current => (current + 1) % welcomeVariants.length);
    }, delay);

    return () => window.clearTimeout(timeoutId);
  }, [isDeletingWelcome, typedWelcome, welcomeVariantIndex, welcomeVariants]);

  return (
    <div className="mx-auto w-full max-w-md" data-walkthrough="home-card">
      {shouldShowBudgetCompletedMessage && (
        <UsageLimitBanner
          tone="danger"
          icon="⚠️"
          title={t('home.usageExhaustedTitle')}
          message={t('home.usageExhaustedBody')}
          ctaLabel={t('home.usageExhaustedCta')}
          secondaryCtaLabel={
            openRouterStatus === 'saving' ? t('openrouterFree.saving') : t('openrouterFree.cta')
          }
          onSecondaryCtaClick={() => {
            if (openRouterStatus !== 'saving') {
              void handleUseOpenRouterFree();
            }
          }}
        />
      )}
      {openRouterStatus === 'error' && (
        <div className="mb-3 rounded-lg border border-coral-200 bg-coral-50 px-3 py-2 text-xs text-coral-700 dark:border-coral-500/30 dark:bg-coral-900/20 dark:text-coral-200">
          {t('openrouterFree.error')}
        </div>
      )}

      {showPromoBanner && <PromotionalCreditsBanner promoCredits={promoCredits} />}

      {/* Animated greeting */}
      <h1 className="min-h-[3.5rem] text-32l font-bold text-stone-900 dark:text-neutral-100 text-center">
        {typedWelcome}
        <span aria-hidden="true" className="ml-0.5 inline-block text-primary-500 animate-pulse">
          |
        </span>
      </h1>

      {/* Connection status — surfaces the broken link when not "ok". */}
      <div className="mb-3 flex justify-center">
        <ConnectionIndicator />
      </div>
      {blocking !== 'ok' && (
        <p className="mb-4 text-center text-sm leading-relaxed text-stone-500 dark:text-neutral-400">
          {statusCopy}
        </p>
      )}

      {/* Recovery: only when the local core is the broken link. */}
      {blocking === 'core-unreachable' && (
        <div className="mb-2">
          <button
            type="button"
            onClick={handleRestartCore}
            disabled={isRestartingCore}
            className="w-full rounded-xl bg-amber-500 py-3 font-medium text-white transition-colors duration-200 hover:bg-amber-600 disabled:opacity-50">
            {isRestartingCore ? t('home.restartingCore') : t('home.restartCore')}
          </button>
          {restartError && (
            <p className="mt-2 text-center text-xs text-coral-500">{restartError}</p>
          )}
        </div>
      )}

      <DiscordBanner />
    </div>
  );
}
