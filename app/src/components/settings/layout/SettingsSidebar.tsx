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

  // Search filters the visible nav tabs in place (rather than rendering a
  // separate result list): a query keeps only entries whose title / id /
  // keywords match, and drops groups that end up empty.
  const [searchQuery, setSearchQuery] = useState('');
  const query = searchQuery.trim().toLowerCase();
  const isSearching = query.length > 0;

  const activeSidebarId = resolveSidebarId(currentRoute);
  const groups = sidebarGroups()
    .map(group => ({
      ...group,
      entries: group.entries.filter(entry => {
        if (!isSearching) return true;
        return (
          t(entry.titleKey).toLowerCase().includes(query) ||
          entry.id.toLowerCase().includes(query) ||
          (entry.searchKeywords ?? []).some(keyword => keyword.toLowerCase().includes(query))
        );
      }),
    }))
    .filter(group => group.entries.length > 0);

  return (
    <nav
      aria-label={t('nav.settings')}
      data-walkthrough="settings-menu"
      className="flex h-full flex-col">
      {/* Full-width search field as a fixed header (no padding). The scroll
          lives on the content below, not on this header. */}
      <SettingsSearchBar value={searchQuery} onValueChange={setSearchQuery} />

      <div className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2">
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

        {isSearching && groups.length === 0 && (
          <p
            data-testid="settings-search-empty"
            className="px-2 pt-3 text-center text-xs text-stone-400 dark:text-neutral-500">
            {t('settings.settingsSearch.noResults').replace('{query}', searchQuery.trim())}
          </p>
        )}
      </div>

      {/* Sticky, centered version footer. */}
      <div className="shrink-0 border-t border-stone-200 py-1 text-center text-[10px] text-stone-400 dark:border-neutral-800 dark:text-neutral-500">
        {t('settings.betaBuild').replace('{version}', APP_VERSION)}
      </div>
    </nav>
  );
};

export default SettingsSidebar;
