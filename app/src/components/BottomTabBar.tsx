import { useEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';

import { useT } from '../lib/i18n/I18nContext';
import { useCoreState } from '../providers/CoreStateProvider';
import { trackEvent } from '../services/analytics';
import {
  loadAgentProfiles,
  selectActiveAgentProfile,
  selectActiveAgentProfileId,
  selectAgentProfile,
  selectAgentProfiles,
} from '../store/agentProfileSlice';
import { selectCompanionSessionActive } from '../store/companionSlice';
import { useAppDispatch, useAppSelector } from '../store/hooks';
import { selectUnreadCount } from '../store/notificationSlice';
import type { AgentProfile } from '../types/agentProfile';
import { isAccountsFullscreen } from '../utils/accountsFullscreen';

const makeTabs = (t: (key: string) => string) => [
  {
    id: 'home',
    label: t('nav.home'),
    path: '/home',
    icon: (
      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={1.8}
          d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-4 0a2 2 0 01-2-2v-4a2 2 0 012-2h2a2 2 0 012 2v4a2 2 0 01-2 2h-2z"
        />
      </svg>
    ),
  },
  {
    id: 'human',
    label: t('nav.human'),
    path: '/human',
    icon: (
      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={1.8}
          d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14c-4 0-7 2.5-7 6h14c0-3.5-3-6-7-6z"
        />
      </svg>
    ),
  },
  {
    id: 'chat',
    label: t('nav.chat'),
    path: '/chat',
    icon: (
      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={1.8}
          d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z"
        />
      </svg>
    ),
  },
  {
    id: 'skills',
    label: t('nav.connections'),
    path: '/skills',
    icon: (
      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={1.8}
          d="M14 10l-2 1m0 0l-2-1m2 1v2.5M20 7l-2 1m2-1l-2-1m2 1v2.5M14 4l-2-1-2 1M4 7l2-1M4 7l2 1M4 7v2.5M12 21l-2-1m2 1l2-1m-2 1v-2.5M6 18l-2-1v-2.5M18 18l2-1v-2.5"
        />
      </svg>
    ),
  },
  {
    id: 'intelligence',
    label: t('nav.memory'),
    path: '/intelligence',
    icon: (
      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={1.8}
          d="M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z"
        />
      </svg>
    ),
  },
  // Alerts/Notifications used to be its own bottom tab; moved into
  // Settings › Notifications since it's a low-traffic destination.
  // The /notifications route still exists for deep links.
  {
    id: 'rewards',
    label: t('nav.rewards'),
    path: '/rewards',
    icon: (
      <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={1.8}
          d="M12 8v8m0-8l-3-3m3 3l3-3M8 14H6a2 2 0 01-2-2V7a2 2 0 012-2h2m8 9h2a2 2 0 002-2V7a2 2 0 00-2-2h-2M7 19h10"
        />
      </svg>
    ),
  },
  {
    id: 'settings',
    label: t('nav.settings'),
    path: '/settings',
    icon: (
      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={1.8}
          d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
        />
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={1.8}
          d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
        />
      </svg>
    ),
  },
];

const getInitials = (name: string): string => {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return 'OH';
  return words
    .slice(0, 2)
    .map(word => word[0]?.toUpperCase() ?? '')
    .join('');
};

function AgentProfileAvatar({
  profile,
  fallbackName,
}: {
  profile: AgentProfile | null;
  fallbackName: string;
}) {
  const name = profile?.name?.trim() || fallbackName;
  if (profile?.avatarUrl) {
    return (
      <img
        src={profile.avatarUrl}
        alt=""
        className="h-6 w-6 rounded-full object-cover"
        referrerPolicy="no-referrer"
      />
    );
  }

  return (
    <span className="flex h-6 w-6 items-center justify-center rounded-full bg-primary-500 text-[10px] font-semibold leading-none text-white">
      {getInitials(name)}
    </span>
  );
}

const BottomTabBar = () => {
  const { t } = useT();
  const location = useLocation();
  const navigate = useNavigate();
  const dispatch = useAppDispatch();
  const { snapshot } = useCoreState();
  const token = snapshot.sessionToken;
  const [revealed, setRevealed] = useState(false);
  const [profileMenuOpen, setProfileMenuOpen] = useState(false);
  const profileMenuRef = useRef<HTMLDivElement>(null);

  const activeAccountId = useAppSelector(state => state.accounts.activeAccountId);
  const unreadCount = useAppSelector(state => selectUnreadCount(state.notifications.items));
  const companionActive = useAppSelector(selectCompanionSessionActive);
  const agentProfiles = useAppSelector(selectAgentProfiles);
  const activeAgentProfileId = useAppSelector(selectActiveAgentProfileId);
  const activeAgentProfile = useAppSelector(selectActiveAgentProfile);
  const agentProfileStatus = useAppSelector(state => state.agentProfiles.status);
  const agentProfileError = useAppSelector(state => state.agentProfiles.error);
  // `state.theme` is undefined in some test fixtures that build a minimal
  // store without the theme slice; default to the historical 'hover' behavior
  // so an absent theme branch can't crash the bar.
  const tabBarLabels = useAppSelector(state => state.theme?.tabBarLabels ?? 'hover');
  const labelsAlwaysVisible = tabBarLabels === 'always';
  const tabs = useMemo(() => makeTabs(t), [t]);
  const fallbackProfileName = t('nav.defaultAgentProfile');

  useEffect(() => {
    if (!token || agentProfiles.length > 0 || agentProfileStatus !== 'idle') return;
    void dispatch(loadAgentProfiles());
  }, [agentProfileStatus, agentProfiles.length, dispatch, token]);

  useEffect(() => {
    if (!profileMenuOpen) return;

    const onPointerDown = (event: PointerEvent) => {
      if (profileMenuRef.current?.contains(event.target as Node)) return;
      setProfileMenuOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setProfileMenuOpen(false);
    };

    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [profileMenuOpen]);

  const hiddenPaths = ['/', '/login'];
  if (
    !token ||
    hiddenPaths.some(path => location.pathname === path || location.pathname.startsWith(`${path}/`))
  ) {
    return null;
  }

  // On /accounts we want as much real estate as possible for the embedded
  // webview — but *only* when a real account (WhatsApp, …) is selected.
  // The Agent entry keeps the tab bar visible so chatting with the agent
  // feels like a normal page. A thin hover strip along the bottom lets
  // the user reveal the bar manually even in fullscreen mode.
  const fullscreen = isAccountsFullscreen(location.pathname, activeAccountId);
  const collapsed = fullscreen && !revealed;

  const isActive = (path: string) => {
    if (path === '/chat') return location.pathname.startsWith('/chat');
    if (path === '/settings/cron-jobs') return location.pathname.startsWith('/settings/cron-jobs');
    if (path === '/settings/messaging') return location.pathname.startsWith('/settings/messaging');
    if (path === '/settings')
      return (
        location.pathname === '/settings' ||
        (location.pathname.startsWith('/settings/') &&
          !location.pathname.startsWith('/settings/cron-jobs') &&
          !location.pathname.startsWith('/settings/messaging'))
      );
    if (path === '/home') return location.pathname === '/home';
    return location.pathname === path;
  };

  const activeTab = tabs.find(tab => isActive(tab.path));

  const handleTabClick = (tab: (typeof tabs)[number], active: boolean) => {
    if (!active) {
      trackEvent('tab_bar_change', {
        from_tab: activeTab?.id ?? 'unknown',
        to_tab: tab.id,
        from_path: location.pathname,
        to_path: tab.path,
      });
    }
    navigate(tab.path);
  };

  const handleProfileSelect = async (profile: AgentProfile) => {
    if (profile.id === activeAgentProfileId) {
      setProfileMenuOpen(false);
      return;
    }

    trackEvent('agent_profile_switch', {
      from_profile_id: activeAgentProfileId,
      to_profile_id: profile.id,
      from_path: location.pathname,
    });

    try {
      await dispatch(selectAgentProfile(profile.id)).unwrap();
      setProfileMenuOpen(false);
    } catch (error) {
      console.warn('[BottomTabBar] agent profile select failed', error);
    }
  };

  const handleRetryProfiles = () => {
    void dispatch(loadAgentProfiles());
  };

  return (
    // pointer-events-none on the full-width shell so transparent areas (e.g.
    // beside the centered nav pill) do not steal clicks from sticky footers
    // such as Settings SaveBar. Only the <nav> pill re-enables hits.
    <div className="pointer-events-none absolute inset-x-0 bottom-0 z-50">
      {/* Hover strip — only matters when collapsed; provides a 12px bottom
          edge the user can mouse into to reveal the bar again. */}
      {collapsed && (
        <div
          className="pointer-events-auto absolute inset-x-0 bottom-0 h-3"
          onMouseEnter={() => setRevealed(true)}
          aria-hidden
        />
      )}
      <div
        className={`pointer-events-none flex justify-center px-4 pb-4 pt-2 transition-transform duration-300 ease-out ${
          collapsed ? 'translate-y-[calc(100%+8px)]' : 'translate-y-0'
        }`}
        onMouseLeave={() => setRevealed(false)}
        onFocus={() => setRevealed(true)}
        onBlur={e => {
          if (!e.currentTarget.contains(e.relatedTarget as Node)) setRevealed(false);
        }}>
        <nav className="pointer-events-auto inline-flex items-center gap-1 rounded-sm border border-stone-300 dark:border-neutral-700 bg-stone-200 dark:bg-neutral-900 shadow-soft px-1 py-1">
          {tabs.map(tab => {
            const active = isActive(tab.path);
            const showBadge = tab.id === 'notifications' && unreadCount > 0;
            const showCompanionDot = tab.id === 'settings' && companionActive;
            // data-walkthrough attributes for the Joyride walkthrough steps.
            // Maps tab ids to their walkthrough target names.
            const walkthroughAttr: Record<string, string> = {
              chat: 'tab-chat',
              skills: 'tab-skills',
              notifications: 'tab-notifications',
              settings: 'tab-settings',
            };
            return (
              <button
                key={tab.id}
                data-walkthrough={walkthroughAttr[tab.id]}
                onClick={() => handleTabClick(tab, active)}
                className={`group relative flex items-center px-2 py-2 rounded-sm text-sm transition-colors duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] cursor-pointer ${
                  active
                    ? 'bg-white dark:bg-neutral-800 text-stone-900 dark:text-neutral-100 font-semibold shadow-sm'
                    : 'bg-transparent text-stone-500 dark:text-neutral-400 hover:bg-stone-300/50 dark:hover:bg-neutral-800/60 hover:text-stone-700 dark:hover:text-neutral-200'
                }`}
                aria-label={
                  tab.id === 'notifications' && unreadCount > 0
                    ? `${tab.label} (${unreadCount} ${t('alerts.unread')})`
                    : tab.label
                }>
                <span className="relative inline-flex flex-shrink-0">
                  {tab.icon}
                  {showBadge && (
                    <span className="absolute -top-1 -right-1 min-w-[14px] h-[14px] px-1 rounded-full bg-coral-500 text-[9px] font-bold text-white flex items-center justify-center leading-none">
                      {unreadCount > 9 ? '9+' : unreadCount}
                    </span>
                  )}
                  {showCompanionDot && (
                    <span className="absolute -top-0.5 -right-0.5 h-2 w-2 rounded-full bg-blue-500 animate-pulse" />
                  )}
                </span>
                <span
                  className={`overflow-hidden whitespace-nowrap transition-[max-width,margin-left,opacity] duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] ${
                    active || labelsAlwaysVisible
                      ? 'max-w-[160px] ml-2 opacity-100'
                      : 'max-w-0 ml-0 opacity-0 group-hover:max-w-[160px] group-hover:ml-2 group-hover:opacity-100 group-focus-visible:max-w-[160px] group-focus-visible:ml-2 group-focus-visible:opacity-100'
                  }`}>
                  {tab.label}
                </span>
              </button>
            );
          })}
          <div
            className="relative ml-1 border-l border-stone-300 pl-1 dark:border-neutral-700"
            ref={profileMenuRef}>
            <button
              type="button"
              onClick={() => setProfileMenuOpen(open => !open)}
              className={`relative flex h-9 w-9 items-center justify-center rounded-sm transition-colors duration-300 cursor-pointer ${
                profileMenuOpen
                  ? 'bg-white text-stone-900 shadow-sm dark:bg-neutral-800 dark:text-neutral-100'
                  : 'bg-transparent text-stone-500 hover:bg-stone-300/50 hover:text-stone-700 dark:text-neutral-400 dark:hover:bg-neutral-800/60 dark:hover:text-neutral-200'
              }`}
              aria-haspopup="menu"
              aria-expanded={profileMenuOpen}
              aria-label={`${t('nav.switchAgentProfile')}: ${
                activeAgentProfile?.name ?? fallbackProfileName
              }`}
              title={activeAgentProfile?.name ?? fallbackProfileName}>
              <AgentProfileAvatar profile={activeAgentProfile} fallbackName={fallbackProfileName} />
              {agentProfileStatus === 'saving' && (
                <span className="absolute inset-1 rounded-sm border border-primary-400/80 animate-pulse" />
              )}
            </button>

            {profileMenuOpen && (
              <div
                role="menu"
                aria-label={t('nav.agentProfiles')}
                className="absolute bottom-full right-0 mb-2 w-64 overflow-hidden rounded-sm border border-stone-300 bg-white shadow-soft dark:border-neutral-700 dark:bg-neutral-900">
                <div className="border-b border-stone-200 px-3 py-2 text-xs font-semibold text-stone-500 dark:border-neutral-800 dark:text-neutral-400">
                  {t('nav.agentProfiles')}
                </div>

                {agentProfileError && (
                  <div className="space-y-2 px-3 py-3 text-sm text-coral-700 dark:text-coral-300">
                    <div>{agentProfileError}</div>
                    <button
                      type="button"
                      onClick={handleRetryProfiles}
                      className="rounded-sm border border-coral-300 px-2 py-1 text-xs font-semibold text-coral-700 hover:bg-coral-50 dark:border-coral-700 dark:text-coral-200 dark:hover:bg-coral-950/40">
                      {t('common.retry')}
                    </button>
                  </div>
                )}

                {!agentProfileError && agentProfileStatus === 'loading' && (
                  <div className="px-3 py-3 text-sm text-stone-500 dark:text-neutral-400">
                    {t('common.loading')}
                  </div>
                )}

                {!agentProfileError &&
                  agentProfileStatus !== 'loading' &&
                  agentProfiles.length === 0 && (
                    <div className="px-3 py-3 text-sm text-stone-500 dark:text-neutral-400">
                      {t('nav.noAgentProfiles')}
                    </div>
                  )}

                {!agentProfileError && agentProfiles.length > 0 && (
                  <div className="max-h-72 overflow-y-auto p-1">
                    {agentProfiles.map(profile => {
                      const active = profile.id === activeAgentProfileId;
                      return (
                        <button
                          key={profile.id}
                          type="button"
                          role="menuitemradio"
                          aria-checked={active}
                          onClick={() => void handleProfileSelect(profile)}
                          className={`flex w-full items-center gap-3 rounded-sm px-2 py-2 text-left transition-colors ${
                            active
                              ? 'bg-primary-50 text-primary-900 dark:bg-primary-950/40 dark:text-primary-100'
                              : 'text-stone-700 hover:bg-stone-100 dark:text-neutral-200 dark:hover:bg-neutral-800'
                          }`}>
                          <AgentProfileAvatar
                            profile={profile}
                            fallbackName={fallbackProfileName}
                          />
                          <span className="min-w-0 flex-1">
                            <span className="block truncate text-sm font-medium">
                              {profile.name}
                            </span>
                            {profile.description && (
                              <span className="block truncate text-xs text-stone-500 dark:text-neutral-400">
                                {profile.description}
                              </span>
                            )}
                          </span>
                          {active && (
                            <span className="h-2 w-2 rounded-full bg-primary-500" aria-hidden />
                          )}
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            )}
          </div>
        </nav>
      </div>
    </div>
  );
};

export default BottomTabBar;
