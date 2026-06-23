import { useLocation, useNavigate } from 'react-router-dom';

import { useT } from '../../../lib/i18n/I18nContext';
import { trackEvent } from '../../../services/analytics';
import { APP_VERSION } from '../../../utils/config';
import ConnectionIndicator from '../../ConnectionIndicator';
import { NavIcon } from './navIcons';
import SidebarAppRail from './SidebarAppRail';
import SidebarHeader from './SidebarHeader';
import SidebarNav from './SidebarNav';
import { SidebarSlotOutlet } from './SidebarSlot';

/**
 * The root-shell sidebar, split top-to-bottom into:
 *
 *   ┌──────────────┐
 *   │ SidebarHeader │  utility row (collapse / settings / language)
 *   ├──────────────┤
 *   │ SidebarNav    │  static primary navigation
 *   ├──────────────┤
 *   │ SidebarAppRail│  persistent app switcher (agent + connected apps)
 *   ├──────────────┤
 *   │ SidebarSlot   │  dynamic, per-route content (scrolls)
 *   │  (Outlet)     │
 *   ├──────────────┤
 *   │ beta footer   │  app-wide build/version line
 *   └──────────────┘
 *
 * Pages project content into the slot region with {@link SidebarContent}.
 * Background matches the previous in-page sidebar pane (white / neutral-900).
 */
export default function AppSidebar() {
  const { t } = useT();
  const location = useLocation();
  const navigate = useNavigate();
  const feedbackActive = location.pathname === '/feedback';

  const handleFeedbackClick = () => {
    if (!feedbackActive) {
      trackEvent('tab_bar_change', {
        from_tab: 'unknown',
        to_tab: 'feedback',
        from_path: location.pathname,
        to_path: '/feedback',
      });
    }
    navigate('/feedback');
  };

  return (
    <div className="flex h-full min-h-0 flex-col bg-white dark:bg-neutral-900">
      <div className="flex-shrink-0 border-b border-stone-200/70 dark:border-neutral-800/70">
        <SidebarHeader />
      </div>
      <div className="flex-shrink-0">
        <SidebarNav />
      </div>
      {/* Persistent app switcher — sticks across routes so the agent + connected
          apps are always one click away. Selecting one routes to /chat where the
          provider webview / agent chat actually render. */}
      <div className="flex-shrink-0 border-t border-stone-200/70 dark:border-neutral-800/70">
        <SidebarAppRail />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto border-t border-stone-200/70 dark:border-neutral-800/70">
        {/* Flex column so routes that project more than one region (e.g. Chat's
            app rail above its thread list) can order them via Tailwind `order-*`. */}
        <SidebarSlotOutlet className="flex h-full flex-col" />
      </div>
      {/* Slim feedback row — pinned just above the status bar. Kept thin and
          low-profile so it reads as a footer affordance, not a primary nav tab
          (it used to live in SidebarNav). */}
      <button
        type="button"
        data-walkthrough="tab-feedback"
        onClick={handleFeedbackClick}
        title={t('nav.feedback')}
        aria-current={feedbackActive ? 'page' : undefined}
        className={`group flex flex-shrink-0 items-center justify-center gap-2 border-t border-stone-200/70 px-3 py-1 text-[11px] transition-colors cursor-pointer dark:border-neutral-800/70 ${
          feedbackActive
            ? 'bg-white text-stone-900 font-medium dark:bg-neutral-800 dark:text-neutral-100'
            : 'text-stone-500 hover:bg-stone-200/70 hover:text-stone-700 dark:text-neutral-400 dark:hover:bg-neutral-800/60 dark:hover:text-neutral-200'
        }`}>
        <NavIcon id="feedback" className="h-3.5 w-3.5 flex-shrink-0" />
        <span className="min-w-0 truncate">{t('nav.feedback')}</span>
      </button>
      {/* App-wide footer: connectivity status + build/version, pinned to the
          bottom of the sidebar. */}
      <div className="flex flex-shrink-0 items-center justify-center gap-2 border-t border-stone-200 px-2 py-0.5 dark:border-neutral-800">
        <ConnectionIndicator />
        &middot;
        <span className="text-[10px] text-stone-400 dark:text-neutral-500">
          {t('settings.betaBuild').replace('{version}', APP_VERSION)}
        </span>
      </div>
    </div>
  );
}
