import { useLocation, useNavigate } from 'react-router-dom';

import { useT } from '../../../lib/i18n/I18nContext';
import Webhooks from '../../../pages/Webhooks';
import PanelPage from '../../layout/PanelPage';
import SettingsBackButton from '../components/SettingsBackButton';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import ComposioPanel from './ComposioPanel';
import TaskSourcesPanel from './TaskSourcesPanel';

type TabId = 'task-sources' | 'composio' | 'webhooks';

const TAB_HASH: Record<TabId, string> = {
  'task-sources': '',
  composio: '#composio',
  webhooks: '#webhooks',
};

const hashToTab = (hash: string): TabId => {
  if (hash === '#composio') return 'composio';
  if (hash === '#webhooks') return 'webhooks';
  return 'task-sources';
};

/**
 * Single Settings entry for integrations. Combines the task-source toggles
 * (TaskSourcesPanel), the Composio routing/auth controls (ComposioPanel) and
 * the webhook trigger history/triage (Webhooks page) as three tabs under one
 * header. The active tab is reflected in the URL hash (`#composio`,
 * `#webhooks`) so deep links and the legacy task-sources/composio-routing/
 * webhooks-triggers redirects land on the right view.
 */
const IntegrationsPanel = () => {
  const { t } = useT();
  const { navigateBack } = useSettingsNavigation();
  const location = useLocation();
  const navigate = useNavigate();
  // The router is the single source of truth for the active tab.
  const tab: TabId = hashToTab(location.hash);

  const selectTab = (next: TabId) => {
    navigate(`${location.pathname}${TAB_HASH[next]}`, { replace: true });
  };

  return (
    <PanelPage<TabId>
      className="z-10"
      description={t('settings.integrations.menuDesc')}
      leading={<SettingsBackButton onBack={navigateBack} />}
      tabsAriaLabel={t('settings.integrations.title')}
      tabsTestIdPrefix="integrations-tab"
      value={tab}
      onChange={selectTab}
      tabs={[
        {
          id: 'task-sources',
          label: t('settings.taskSources.title'),
          content: <TaskSourcesPanel embedded />,
        },
        {
          id: 'composio',
          label: t('settings.developerMenu.composioRouting.title'),
          content: <ComposioPanel embedded />,
          contentClassName: 'p-4',
        },
        {
          id: 'webhooks',
          label: t('settings.developerMenu.composeioTriggers.title'),
          content: <Webhooks embedded />,
        },
      ]}
    />
  );
};

export default IntegrationsPanel;
