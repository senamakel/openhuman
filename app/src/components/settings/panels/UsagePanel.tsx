import { useEffect, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';

import { useT } from '../../../lib/i18n/I18nContext';
import { type AISettings, loadAISettings } from '../../../services/api/aiSettingsApi';
import CostDashboardPanel from '../../dashboard/CostDashboardPanel';
import ChipTabs from '../../layout/ChipTabs';
import PanelScaffold from '../../layout/PanelScaffold';
import SettingsBackButton from '../components/SettingsBackButton';
import { SettingsStatusLine } from '../controls';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import { BackgroundLoopControls } from './AIPanel';

type TabId = 'costs' | 'background';

const TAB_HASH: Record<TabId, string> = { costs: '', background: '#background' };

const hashToTab = (hash: string): TabId => (hash === '#background' ? 'background' : 'costs');

/**
 * Single Settings entry for usage & limits. Combines the cost dashboard
 * (charts, budgets, usage log) and the background-activity controls
 * (heartbeat cadences + usage ledger, previously the separate Heartbeat and
 * Usage-ledger pages) as two tabs under one header. The active tab is
 * reflected in the URL hash (`#background`) so deep links and the legacy
 * heartbeat/ledger-usage redirects land on the right view.
 */
const UsagePanel = () => {
  const { t } = useT();
  const { navigateBack } = useSettingsNavigation();
  const location = useLocation();
  const navigate = useNavigate();
  // The router is the single source of truth for the active tab.
  const tab: TabId = hashToTab(location.hash);

  const selectTab = (next: TabId) => {
    navigate(`${location.pathname}${TAB_HASH[next]}`, { replace: true });
  };

  const tabs: { id: TabId; label: string }[] = [
    { id: 'costs', label: t('settings.costDashboard.title') },
    { id: 'background', label: t('settings.heartbeat.title') },
  ];

  return (
    <PanelScaffold
      className="z-10"
      contentClassName=""
      title={t('settings.usage.title')}
      leading={<SettingsBackButton onBack={navigateBack} />}
      headerExtra={
        <ChipTabs
          ariaLabel={t('settings.usage.title')}
          testIdPrefix="usage-tab"
          items={tabs}
          value={tab}
          onChange={selectTab}
        />
      }>
      {tab === 'costs' ? <CostDashboardPanel embedded /> : <BackgroundActivityTab />}
    </PanelScaffold>
  );
};

/**
 * Background-activity tab body. Fetches the AI settings snapshot (routing map
 * + cloud providers) that BackgroundLoopControls needs — lazily, only when
 * this tab is mounted, so the default Costs tab doesn't pay for it.
 */
const BackgroundActivityTab = () => {
  const { t } = useT();
  const [snapshot, setSnapshot] = useState<AISettings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    loadAISettings()
      .then(s => {
        if (!cancelled) setSnapshot(s);
      })
      .catch(err => {
        if (!cancelled) setLoadError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="p-4 space-y-3" data-testid="usage-background-tab">
      <SettingsStatusLine saving={false} error={loadError} savingLabel="" />
      {snapshot ? (
        <BackgroundLoopControls
          view="all"
          hideHeader
          routing={snapshot.routing}
          cloudProviders={snapshot.cloudProviders}
        />
      ) : !loadError ? (
        <div className="text-xs text-neutral-500 dark:text-neutral-400">{t('common.loading')}</div>
      ) : null}
    </div>
  );
};

export default UsagePanel;
