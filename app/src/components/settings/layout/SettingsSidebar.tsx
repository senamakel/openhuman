import { useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import { APP_VERSION } from '../../../utils/config';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import SettingsSearchBar from '../search/SettingsSearchBar';
import {
  entryRoute,
  NAV_GROUP_LABEL_KEY,
  resolveSidebarId,
  sidebarGroups,
} from '../settingsRouteRegistry';
import { SETTINGS_NAV_ICONS } from './settingsNavIcons';

/**
 * Grouped settings navigation. On wide viewports this is the persistent left
 * pane of the two-pane layout; on narrow viewports it doubles as the
 * /settings index page (the old drill-down home list).
 */
const SettingsSidebar = () => {
  const { t } = useT();
  const { currentRoute, navigateToSettings } = useSettingsNavigation();

  // Global settings search. While a query is active the nav list is hidden —
  // the search bar renders its own ranked result list instead.
  const [searchQuery, setSearchQuery] = useState('');
  const isSearching = searchQuery.trim().length > 0;

  const activeSidebarId = resolveSidebarId(currentRoute);
  const groups = sidebarGroups();

  return (
    <nav aria-label={t('nav.settings')} data-walkthrough="settings-menu" className="flex flex-col">
      {/* Search bar pinned to the top of the scrolling sidebar pane. */}
      <div className="sticky top-0 z-10 bg-white px-1.5 pb-1 pt-1.5 dark:bg-neutral-900">
        <SettingsSearchBar value={searchQuery} onValueChange={setSearchQuery} />
      </div>

      {!isSearching && (
        <div className="px-1.5 pb-2">
          {groups.map(({ group, entries }) => (
            <div key={group} data-testid={`settings-sidebar-group-${group}`}>
              <div className="px-2 pb-0.5 pt-2.5">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-stone-500 dark:text-neutral-400">
                  {t(NAV_GROUP_LABEL_KEY[group])}
                </span>
              </div>
              <ul>
                {entries.map(entry => {
                  const active = activeSidebarId === entry.id;
                  return (
                    <li key={entry.id}>
                      <button
                        type="button"
                        data-testid={`settings-nav-${entry.id}`}
                        aria-current={active ? 'page' : undefined}
                        onClick={() => navigateToSettings(entryRoute(entry))}
                        className={`flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-[13px] transition-colors ${
                          active
                            ? 'bg-stone-100 font-medium text-stone-900 dark:bg-neutral-800 dark:text-neutral-100'
                            : 'text-stone-600 hover:bg-stone-50 hover:text-stone-900 dark:text-neutral-300 dark:hover:bg-neutral-800/60 dark:hover:text-neutral-100'
                        }`}>
                        <span
                          className={`shrink-0 ${
                            active
                              ? 'text-primary-600 dark:text-primary-400'
                              : 'text-stone-400 dark:text-neutral-500'
                          }`}>
                          {SETTINGS_NAV_ICONS[entry.id] ?? null}
                        </span>
                        <span className="truncate">{t(entry.titleKey)}</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </div>
          ))}

          <div className="mt-3 border-t border-stone-200 px-2 pb-1 pt-2 text-[10px] text-stone-400 dark:border-neutral-800 dark:text-neutral-500">
            {t('settings.betaBuild').replace('{version}', APP_VERSION)}
          </div>
        </div>
      )}
    </nav>
  );
};

export default SettingsSidebar;
