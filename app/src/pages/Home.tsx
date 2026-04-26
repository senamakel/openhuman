import { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import ConnectionIndicator from '../components/ConnectionIndicator';
import { useUsageState } from '../hooks/useUsageState';
import { useUser } from '../hooks/useUser';
import { useAppSelector } from '../store/hooks';
import { selectSocketStatus } from '../store/socketSelectors';
import { APP_VERSION } from '../utils/config';
import { DISCORD_INVITE_URL } from '../utils/links';

export function resolveHomeUserName(user: unknown): string {
  if (!user || typeof user !== 'object') return 'User';

  const record = user as Record<string, unknown>;
  const firstName =
    (typeof record.firstName === 'string' && record.firstName.trim()) ||
    (typeof record.first_name === 'string' && record.first_name.trim()) ||
    '';
  const lastName =
    (typeof record.lastName === 'string' && record.lastName.trim()) ||
    (typeof record.last_name === 'string' && record.last_name.trim()) ||
    '';
  const username = typeof record.username === 'string' ? record.username.trim() : '';
  const email = typeof record.email === 'string' ? record.email.trim() : '';

  const fullName = [firstName, lastName].filter(Boolean).join(' ').trim();
  if (fullName) return fullName;
  if (firstName) return firstName;
  if (username) return username.startsWith('@') ? username : `@${username}`;
  if (email) return email.split('@')[0] || 'User';
  return 'User';
}

function formatUsd(amount: number): string {
  return `$${amount.toFixed(amount % 1 === 0 ? 0 : 2)}`;
}

function HomeBanner({
  tone,
  icon,
  title,
  message,
  ctaLabel,
  onCtaClick,
}: {
  tone: 'warning' | 'danger';
  icon: string;
  title: string;
  message: string;
  ctaLabel: string;
  onCtaClick: () => void;
}) {
  const styles =
    tone === 'danger'
      ? {
          card: 'border-coral-200 bg-gradient-to-r from-coral-50 via-rose-50 to-orange-50',
          iconWrap: 'bg-coral-100',
          title: 'text-stone-900',
          body: 'text-stone-600',
          button:
            'border-coral-200 bg-white text-coral-700 hover:border-coral-300 hover:bg-coral-50',
        }
      : {
          card: 'border-amber-200 bg-gradient-to-r from-amber-50 via-orange-50 to-rose-50',
          iconWrap: 'bg-amber-100',
          title: 'text-stone-900',
          body: 'text-stone-600',
          button:
            'border-amber-200 bg-white text-amber-700 hover:border-amber-300 hover:bg-amber-50',
        };

  return (
    <div className={`mb-3 rounded-2xl border px-4 py-4 text-left shadow-soft ${styles.card}`}>
      <div className="flex items-start gap-3">
        <div
          className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-lg ${styles.iconWrap}`}>
          {icon}
        </div>
        <div className="min-w-0 flex-1">
          <p className={`text-sm font-semibold ${styles.title}`}>{title}</p>
          <p className={`mt-1 text-sm leading-relaxed ${styles.body}`}>{message}</p>
          <button
            onClick={onCtaClick}
            className={`mt-3 rounded-lg border px-3 py-2 text-xs font-medium transition-colors ${styles.button}`}>
            {ctaLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

const Home = () => {
  const { user } = useUser();
  const navigate = useNavigate();
  const { isRateLimited, shouldShowBudgetCompletedMessage } = useUsageState();
  const _userName = resolveHomeUserName(user);
  const userName = _userName.split(' ')[0]; // Get first name only
  const promoCredits = user?.usage?.promotionBalanceUsd ?? 0;
  const showPromoBanner =
    user?.subscription?.plan === 'FREE' &&
    user?.subscription?.hasActiveSubscription === false &&
    promoCredits > 0;
  const welcomeVariants = useMemo(
    () => [`Welcome, ${userName} 👋`, `Let's cook, ${userName} 🧑‍🍳.`, `Time to Zone In 🧘🏻`],
    [userName]
  );
  const [welcomeVariantIndex, setWelcomeVariantIndex] = useState(0);
  const [typedWelcome, setTypedWelcome] = useState('');
  const [isDeletingWelcome, setIsDeletingWelcome] = useState(false);
  // Mirror the same socket status the `ConnectionIndicator` pill consumes
  // so the description copy below the pill never contradicts it (the old
  // hard-coded "connected" message lied while the pill said "Connecting"
  // / "Disconnected").
  const socketStatus = useAppSelector(selectSocketStatus);
  const statusCopy = {
    connected:
      'Your device is connected. Keep the app running to keep the connection alive. Message your assistant with the button below.',
    connecting: 'Connecting. Hang tight, this usually takes a second.',
    disconnected:
      'Your device is offline right now. Check your network or restart the app to reconnect.',
  }[socketStatus];

  // Open in-app chat.
  const handleStartCooking = async () => {
    navigate('/chat');
  };

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
    <div className="min-h-full flex flex-col items-center justify-center p-4">
      <div className="max-w-md w-full">
        {isRateLimited && (
          <HomeBanner
            tone="warning"
            icon="⏳"
            title="You’ve hit a rate limit"
            message="You’ve reached your short-term usage cap. Buy top-up credits to keep going right away."
            ctaLabel="Buy top-up credits"
            onCtaClick={() => navigate('/settings/billing')}
          />
        )}

        {!isRateLimited && shouldShowBudgetCompletedMessage && (
          <HomeBanner
            tone="danger"
            icon="⚡"
            title="You’ve exhausted your usage"
            message="You’re out of included usage for now. Start a subscription to unlock more ongoing capacity."
            ctaLabel="Get a subscription"
            onCtaClick={() => navigate('/settings/billing')}
          />
        )}

        {showPromoBanner && (
          <div className="mb-3 rounded-2xl border border-amber-200 bg-gradient-to-r from-amber-50 via-orange-50 to-rose-50 px-4 py-4 text-left shadow-soft">
            <div className="flex items-start gap-3">
              <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-amber-100 text-lg">
                🎉
              </div>
              <div className="min-w-0 flex-1">
                <p className="text-sm font-semibold text-stone-900">
                  You have {formatUsd(promoCredits)} of promotional credits.
                </p>
                <p className="mt-1 text-sm leading-relaxed text-stone-600">
                  <span>
                    Give OpenHuman a spin, and when you&apos;re ready for more,{' '}
                    <span
                      onClick={() => navigate('/settings/billing')}
                      className="font-bold cursor-pointer text-amber-700 border-b border-amber-700 border-dashed">
                      get a subscription
                    </span>{' '}
                    and get 10x more usage .
                  </span>
                </p>
              </div>
            </div>
          </div>
        )}

        {/* Main card */}
        <div className="bg-white rounded-2xl shadow-soft border border-stone-200 p-6 animate-fade-up">
          {/* Header row: logo + version + settings */}
          <div className="flex items-center justify-center mb-4">
            <span className="text-xs text-center text-stone-400">v{APP_VERSION}</span>
          </div>

          {/* Welcome title */}
          <h1 className="min-h-[3.5rem] text-32l font-bold text-stone-900 text-center">
            {typedWelcome}
            <span aria-hidden="true" className="ml-0.5 inline-block text-primary-500 animate-pulse">
              |
            </span>
          </h1>

          {/* Connection status */}
          <div className="flex justify-center mb-3">
            <ConnectionIndicator />
          </div>

          {/* Description — mirrors the pill's socket status to avoid
              telling the user they're connected while the pill shows
              "Connecting" / "Disconnected". */}
          <p className="text-sm text-stone-500 text-center mb-6 leading-relaxed">{statusCopy}</p>

          {/* CTA button */}
          <button
            onClick={handleStartCooking}
            className="w-full py-3 bg-primary-500 hover:bg-primary-600 text-white font-medium rounded-xl transition-colors duration-200">
            Message OpenHuman
          </button>
        </div>

        <a
          href={DISCORD_INVITE_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="mb-3 mt-3 block rounded-2xl border border-[#CDD2FF] bg-gradient-to-r from-[#F6F7FF] via-[#F1F3FF] to-[#ECEFFF] px-4 py-4 text-[#414AAE] shadow-soft transition-transform transition-colors hover:-translate-y-0.5 hover:border-[#BCC3FF] hover:from-[#EEF0FF] hover:to-[#E5E9FF]">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-[#5865F2]/12 text-[#5865F2]">
              <svg className="h-5 w-5" fill="currentColor" viewBox="0 0 24 24" aria-hidden="true">
                <path d="M20.317 4.37A19.79 19.79 0 0 0 15.885 3c-.191.328-.403.775-.552 1.124a18.27 18.27 0 0 0-5.29 0A11.56 11.56 0 0 0 9.49 3a19.74 19.74 0 0 0-4.433 1.37C2.253 8.51 1.492 12.55 1.872 16.533a19.9 19.9 0 0 0 5.239 2.673c.423-.58.8-1.196 1.123-1.845a12.84 12.84 0 0 1-1.767-.85c.148-.106.292-.217.43-.332c3.408 1.6 7.104 1.6 10.472 0c.14.115.283.226.43.332c-.565.338-1.157.623-1.771.851c.322.648.698 1.264 1.123 1.844a19.84 19.84 0 0 0 5.241-2.673c.446-4.617-.761-8.621-3.787-12.164ZM9.46 14.088c-1.02 0-1.855-.936-1.855-2.084c0-1.148.82-2.084 1.855-2.084c1.044 0 1.87.944 1.855 2.084c0 1.148-.82 2.084-1.855 2.084Zm5.08 0c-1.02 0-1.855-.936-1.855-2.084c0-1.148.82-2.084 1.855-2.084c1.044 0 1.87.944 1.855 2.084c0 1.148-.812 2.084-1.855 2.084Z" />
              </svg>
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-sm font-semibold">Join Our Discord</div>
              <div className="mt-0.5 text-sm text-[#5E66BC]">
                Get updates, free merch, credits, report bugs, and be part of the OpenHuman
                community.
              </div>
            </div>
          </div>
        </a>

        {/* Next steps — compact directory of where to go next */}
        {/* <div className="mt-3 bg-white rounded-2xl shadow-soft border border-stone-200 p-4">
          <div className="text-[11px] uppercase tracking-wide text-stone-400 mb-2">Next steps</div>
          <div className="divide-y divide-stone-100">
            <button
              onClick={() => navigate('/skills')}
              className="w-full flex items-center justify-between py-2.5 text-left hover:bg-stone-50 rounded-md px-2 -mx-2 transition-colors">
              <div>
                <div className="text-sm font-medium text-stone-900">Connect your services</div>
                <div className="text-xs text-stone-500">
                  Give your assistant access to Gmail, Calendar, and more.
                </div>
              </div>
              <svg
                className="w-4 h-4 text-stone-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24">
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M9 5l7 7-7 7"
                />
              </svg>
            </button>
            <button
              onClick={() => navigate('/rewards')}
              className="w-full flex items-center justify-between py-2.5 text-left hover:bg-stone-50 rounded-md px-2 -mx-2 transition-colors">
              <div>
                <div className="text-sm font-medium text-stone-900">Earn rewards</div>
                <div className="text-xs text-stone-500">
                  Unlock credits by using OpenHuman and completing milestones.
                </div>
              </div>
              <svg
                className="w-4 h-4 text-stone-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24">
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M9 5l7 7-7 7"
                />
              </svg>
            </button>
            <button
              onClick={() => navigate('/invites')}
              className="w-full flex items-center justify-between py-2.5 text-left hover:bg-stone-50 rounded-md px-2 -mx-2 transition-colors">
              <div>
                <div className="text-sm font-medium text-stone-900">Invite a friend</div>
                <div className="text-xs text-stone-500">
                  Share an invite — both of you get credits.
                </div>
              </div>
              <svg
                className="w-4 h-4 text-stone-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24">
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M9 5l7 7-7 7"
                />
              </svg>
            </button>
          </div>
        </div> */}
      </div>
    </div>
  );
};

export default Home;
