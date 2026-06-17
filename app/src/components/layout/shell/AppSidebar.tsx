import { useT } from '../../../lib/i18n/I18nContext';
import { APP_VERSION } from '../../../utils/config';
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
  return (
    <div className="flex h-full min-h-0 flex-col bg-white dark:bg-neutral-900">
      <div className="flex-shrink-0 border-b border-stone-200/70 dark:border-neutral-800/70">
        <SidebarHeader />
      </div>
      <div className="flex-shrink-0">
        <SidebarNav />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto border-t border-stone-200/70 dark:border-neutral-800/70">
        {/* Flex column so routes that project more than one region (e.g. Chat's
            app rail above its thread list) can order them via Tailwind `order-*`. */}
        <SidebarSlotOutlet className="flex h-full flex-col" />
      </div>
      {/* App-wide build/version footer, pinned to the bottom of the sidebar. */}
      <div className="flex-shrink-0 border-t border-stone-200 py-1 text-center text-[10px] text-stone-400 dark:border-neutral-800 dark:text-neutral-500">
        {t('settings.betaBuild').replace('{version}', APP_VERSION)}
      </div>
    </div>
  );
}
