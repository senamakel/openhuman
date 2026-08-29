import { useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import TwoPaneNav from '../../layout/TwoPaneNav';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import SettingsSearchBar from '../search/SettingsSearchBar';
import { useSettingsSearch } from '../search/useSettingsSearch';
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
 *
 * Rendered with the shared {@link TwoPaneNav} — the same primitive every other
 * page's sidebar uses, and the same column it is projected into. It used to
 * hand-roll an equivalent row list, which drifted the moment either side was
 * touched: shorter rows (`py-1` vs `py-1.5`), a different group-heading inset,
 * and a grey icon left sitting on the accent fill of a selected row.
 */
const SettingsSidebar = () => {
  const { t } = useT();
  const { currentRoute, navigateToSettings } = useSettingsNavigation();

  // While searching we render a flat, ranked result list backed by the FULL
  // route registry (via useSettingsSearch) — not just the top-level sidebar
  // entries — so deep/sub-nav destinations (privacy, security, agent-access, …)
  // remain reachable via search. With no query we render the grouped nav.
  const [searchQuery, setSearchQuery] = useState('');
  const isSearching = searchQuery.trim().length > 0;
  const searchResults = useSettingsSearch(searchQuery);

  const activeSidebarId = resolveSidebarId(currentRoute);

  // `route` is not carried on the nav item, so keep the id → route map beside
  // the groups and resolve on select.
  const routeById = new Map<string, string>();

  const groups = isSearching
    ? [
        {
          testId: 'settings-search-results',
          items: searchResults.map(result => {
            routeById.set(result.entry.id, result.entry.route);
            return {
              value: result.entry.id,
              label: result.title,
              icon: SETTINGS_NAV_ICONS[result.entry.id] ?? null,
              testId: `settings-nav-${result.entry.id}`,
            };
          }),
        },
      ]
    : sidebarGroups().map(group => ({
        label: t(NAV_GROUP_LABEL_KEY[group.group]),
        testId: `settings-sidebar-group-${group.group}`,
        items: group.entries.map(entry => {
          routeById.set(entry.id, entryRoute(entry));
          return {
            value: entry.id,
            label: t(entry.titleKey),
            icon: SETTINGS_NAV_ICONS[entry.id] ?? null,
            highlight: entry.highlight,
            testId: `settings-nav-${entry.id}`,
          };
        }),
      }));

  const hasRows = groups.some(group => group.items.length > 0);

  return (
    <TwoPaneNav
      ariaLabel={t('nav.settings')}
      walkthroughId="settings-menu"
      // Full-width search field as a fixed header; the scroll lives on the nav
      // list below it, not on the header.
      header={<SettingsSearchBar value={searchQuery} onValueChange={setSearchQuery} />}
      groups={groups}
      selected={activeSidebarId ?? ''}
      onSelect={id => {
        const route = routeById.get(id);
        if (route) navigateToSettings(route);
      }}
      footer={
        isSearching && !hasRows ? (
          <p
            data-testid="settings-search-empty"
            className="px-2.5 pt-3 text-center text-xs text-content-faint">
            {t('settings.settingsSearch.noResults').replace('{query}', searchQuery.trim())}
          </p>
        ) : null
      }
    />
  );
};

export default SettingsSidebar;
