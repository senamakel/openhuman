import { useLocation, useNavigate } from 'react-router-dom';

import { useT } from '../../../lib/i18n/I18nContext';
import ChipTabs from '../../layout/ChipTabs';
import PanelScaffold from '../../layout/PanelScaffold';
import SettingsBackButton from '../components/SettingsBackButton';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import NotificationRoutingPanel from './NotificationRoutingPanel';
import NotificationsPanel from './NotificationsPanel';

type TabId = 'preferences' | 'routing';

const TAB_HASH: Record<TabId, string> = { preferences: '', routing: '#routing' };

const hashToTab = (hash: string): TabId => (hash === '#routing' ? 'routing' : 'preferences');

/**
 * Single Settings entry for notifications. Combines the user-facing
 * preferences (NotificationsPanel) and the routing/intelligence pipeline
 * controls (NotificationRoutingPanel) as two tabs under one header. The
 * active tab is reflected in the URL hash (`#routing`) so deep links from
 * Developer Options still land on the right view.
 */
const NotificationsTabbedPanel = () => {
  const { t } = useT();
  const { navigateBack } = useSettingsNavigation();
  const location = useLocation();
  const navigate = useNavigate();
  // The router is the single source of truth for the active tab — hash is the
  // only signal needed, so derive directly instead of mirroring it in state.
  const tab: TabId = hashToTab(location.hash);

  const selectTab = (next: TabId) => {
    navigate(`${location.pathname}${TAB_HASH[next]}`, { replace: true });
  };

  const tabs: { id: TabId; label: string }[] = [
    { id: 'preferences', label: t('settings.notifications.tabs.preferences') },
    { id: 'routing', label: t('settings.notifications.tabs.routing') },
  ];

  return (
    <PanelScaffold
      className="z-10"
      contentClassName=""
      title={t('settings.notifications')}
      leading={<SettingsBackButton onBack={navigateBack} />}
      headerExtra={
        <ChipTabs
          ariaLabel={t('settings.notifications')}
          items={tabs}
          value={tab}
          onChange={selectTab}
        />
      }>
      {tab === 'preferences' ? (
        <NotificationsPanel embedded />
      ) : (
        <NotificationRoutingPanel embedded />
      )}
    </PanelScaffold>
  );
};

export default NotificationsTabbedPanel;
