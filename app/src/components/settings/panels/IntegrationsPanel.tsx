import { Navigate, useLocation } from 'react-router-dom';

import TaskSourcesPanel from './TaskSourcesPanel';

/**
 * Single Settings entry for integrations. Hosts the task-source toggles
 * (TaskSourcesPanel). The webhook trigger history/triage was retired from the
 * UI; Composio (API key + routing) moved to Connections → API keys.
 */
const IntegrationsPanel = () => {
  const location = useLocation();

  // Legacy deep link: the old Composio tab lived at `#composio` on this page.
  // It now lives under Connections → API keys, so normalize the bookmark.
  if (location.hash === '#composio') {
    return <Navigate to="/connections?tab=composio-key" replace />;
  }

  return <TaskSourcesPanel />;
};

export default IntegrationsPanel;
