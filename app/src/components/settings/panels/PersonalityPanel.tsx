import { useLocation, useNavigate } from 'react-router-dom';

import { useT } from '../../../lib/i18n/I18nContext';
import ChipTabs from '../../layout/ChipTabs';
import PanelScaffold from '../../layout/PanelScaffold';
import SettingsBackButton from '../components/SettingsBackButton';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';
import MascotPanel from './MascotPanel';
import PersonaPanel from './PersonaPanel';

type TabId = 'personality' | 'face';

const TAB_HASH: Record<TabId, string> = { personality: '', face: '#face' };

const hashToTab = (hash: string): TabId => (hash === '#face' ? 'face' : 'personality');

/**
 * Single Settings entry for the assistant's character. Combines the persona
 * editor (PersonaPanel) and the face/mascot picker (MascotPanel, previously
 * the separate /settings/mascot page) as two tabs under one header. The
 * active tab is reflected in the URL hash (`#face`) so deep links and the
 * legacy persona/mascot redirects land on the right view.
 */
const PersonalityPanel = () => {
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
    { id: 'personality', label: t('settings.assistant.personality') },
    { id: 'face', label: t('settings.assistant.faceMascot') },
  ];

  return (
    <PanelScaffold
      className="z-10"
      contentClassName=""
      title={t('settings.personalityFace.title')}
      leading={<SettingsBackButton onBack={navigateBack} />}
      headerExtra={
        <ChipTabs
          ariaLabel={t('settings.personalityFace.title')}
          testIdPrefix="personality-tab"
          items={tabs}
          value={tab}
          onChange={selectTab}
        />
      }>
      {tab === 'personality' ? <PersonaPanel embedded /> : <MascotPanel embedded />}
    </PanelScaffold>
  );
};

export default PersonalityPanel;
