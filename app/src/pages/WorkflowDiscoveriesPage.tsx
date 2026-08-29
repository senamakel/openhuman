/**
 * WorkflowDiscoveriesPage — the dedicated home for Flow Scout's proactive,
 * buildable workflow suggestions. Previously these rendered inline on the
 * Workflows list page; they now live on their own sidebar-reachable page so the
 * list stays focused on the user's saved workflows.
 */
import SuggestedWorkflows from '../components/flows/SuggestedWorkflows';
import SettingsTabbedPage from '../components/settings/layout/SettingsTabbedPage';
import { useT } from '../lib/i18n/I18nContext';

export default function WorkflowDiscoveriesPage() {
  const { t } = useT();
  return (
    <div className="h-full p-4">
      <SettingsTabbedPage
        title={t('flows.discoveries.title')}
        description={t('flows.discoveries.description')}>
        <div className="pt-4">
          <SuggestedWorkflows />
        </div>
      </SettingsTabbedPage>
    </div>
  );
}
