import type { ReactNode } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';

import WorkflowsTab from '../components/intelligence/WorkflowsTab';
import SettingsIndexRedirect from '../components/settings/layout/SettingsIndexRedirect';
import SettingsLayout from '../components/settings/layout/SettingsLayout';
import AboutPanel from '../components/settings/panels/AboutPanel';
import AccountPanel from '../components/settings/panels/AccountPanel';
import AgentAccessPanel from '../components/settings/panels/AgentAccessPanel';
import AgentActivityPanel from '../components/settings/panels/AgentActivityPanel';
import AgentChatPanel from '../components/settings/panels/AgentChatPanel';
import AgentEditorPage from '../components/settings/panels/AgentEditorPage';
import AgentsPanel from '../components/settings/panels/AgentsPanel';
import AIPanel from '../components/settings/panels/AIPanel';
import AnalysisViewsPanel from '../components/settings/panels/AnalysisViewsPanel';
import AppearancePanel from '../components/settings/panels/AppearancePanel';
import ApprovalHistoryPanel from '../components/settings/panels/ApprovalHistoryPanel';
import AutocompleteDebugPanel from '../components/settings/panels/AutocompleteDebugPanel';
import AutocompletePanel from '../components/settings/panels/AutocompletePanel';
import BillingPanel from '../components/settings/panels/BillingPanel';
import CompanionPanel from '../components/settings/panels/CompanionPanel';
import ComposioTriagePanel from '../components/settings/panels/ComposioTriagePanel';
import CronJobsPanel from '../components/settings/panels/CronJobsPanel';
import DeveloperOptionsPanel from '../components/settings/panels/DeveloperOptionsPanel';
import DevicesPanel from '../components/settings/panels/DevicesPanel';
import DevWorkflowPanel from '../components/settings/panels/DevWorkflowPanel';
import EmbeddingsPanel from '../components/settings/panels/EmbeddingsPanel';
import EventLogPanel from '../components/settings/panels/EventLogPanel';
import IntegrationsPanel from '../components/settings/panels/IntegrationsPanel';
import LocalModelDebugPanel from '../components/settings/panels/LocalModelDebugPanel';
import McpServerPanel from '../components/settings/panels/McpServerPanel';
import MemoryDataPanel from '../components/settings/panels/MemoryDataPanel';
import MemoryDebugPanel from '../components/settings/panels/MemoryDebugPanel';
import MemorySyncPanel from '../components/settings/panels/MemorySyncPanel';
import MigrationPanel from '../components/settings/panels/MigrationPanel';
import ModelHealthPanel from '../components/settings/panels/ModelHealthPanel';
import NotificationsTabbedPanel from '../components/settings/panels/NotificationsTabbedPanel';
import PermissionsPanel from '../components/settings/panels/PermissionsPanel';
import PersonalityPanel from '../components/settings/panels/PersonalityPanel';
import PrivacyPanel from '../components/settings/panels/PrivacyPanel';
import RecoveryPhrasePanel from '../components/settings/panels/RecoveryPhrasePanel';
import SandboxSettingsPanel from '../components/settings/panels/SandboxSettingsPanel';
import ScreenAwarenessDebugPanel from '../components/settings/panels/ScreenAwarenessDebugPanel';
import ScreenIntelligencePanel from '../components/settings/panels/ScreenIntelligencePanel';
import SearchPanel from '../components/settings/panels/SearchPanel';
import SecurityPanel from '../components/settings/panels/SecurityPanel';
import TasksPanel from '../components/settings/panels/TasksPanel';
import TeamInvitesPanel from '../components/settings/panels/TeamInvitesPanel';
import TeamManagementPanel from '../components/settings/panels/TeamManagementPanel';
import TeamMembersPanel from '../components/settings/panels/TeamMembersPanel';
import TeamPanel from '../components/settings/panels/TeamPanel';
import ToolPolicyDiagnosticsPanel from '../components/settings/panels/ToolPolicyDiagnosticsPanel';
import ToolsPanel from '../components/settings/panels/ToolsPanel';
import UsagePanel from '../components/settings/panels/UsagePanel';
import VoiceDebugPanel from '../components/settings/panels/VoiceDebugPanel';
import VoicePanel from '../components/settings/panels/VoicePanel';
import WalletBalancesPanel from '../components/settings/panels/WalletBalancesPanel';
import WebhooksDebugPanel from '../components/settings/panels/WebhooksDebugPanel';
import WorkflowRunnerPanel from '../components/settings/panels/WorkflowRunnerPanel';
import Intelligence from './Intelligence';

const WrappedSettingsPage = ({
  children,
  maxWidthClass = 'max-w-2xl',
}: {
  children: ReactNode;
  maxWidthClass?: string;
}) => {
  return (
    <div className="p-4 pt-4">
      <div
        className={`${maxWidthClass} mx-auto bg-white dark:bg-neutral-900 rounded-2xl shadow-soft border border-stone-200 dark:border-neutral-800 overflow-hidden`}>
        {children}
      </div>
    </div>
  );
};

/**
 * Settings routes, hosted inside the two-pane SettingsLayout (persistent
 * sidebar on md+, drill-down on narrow viewports). Retired slugs are kept as
 * redirects so deep links keep working.
 */
const Settings = () => {
  const wrapSettingsPage = (element: ReactNode, opts?: { maxWidthClass?: string }) => (
    <WrappedSettingsPage maxWidthClass={opts?.maxWidthClass}>{element}</WrappedSettingsPage>
  );

  return (
    <div>
      <Routes>
        <Route element={<SettingsLayout />}>
          <Route index element={<SettingsIndexRedirect />} />

          {/* ── General ─────────────────────────────────────────────── */}
          <Route path="account" element={wrapSettingsPage(<AccountPanel />)} />
          <Route path="team" element={wrapSettingsPage(<TeamPanel />)} />
          <Route path="team/manage/:teamId" element={wrapSettingsPage(<TeamManagementPanel />)} />
          <Route
            path="team/manage/:teamId/members"
            element={wrapSettingsPage(<TeamMembersPanel />)}
          />
          <Route
            path="team/manage/:teamId/invites"
            element={wrapSettingsPage(<TeamInvitesPanel />)}
          />
          <Route path="team/members" element={wrapSettingsPage(<TeamMembersPanel />)} />
          <Route path="team/invites" element={wrapSettingsPage(<TeamInvitesPanel />)} />
          <Route path="billing" element={wrapSettingsPage(<BillingPanel />)} />
          <Route path="privacy" element={wrapSettingsPage(<PrivacyPanel />)} />
          <Route path="security" element={wrapSettingsPage(<SecurityPanel />)} />
          <Route path="migration" element={wrapSettingsPage(<MigrationPanel />)} />
          <Route path="appearance" element={wrapSettingsPage(<AppearancePanel />)} />
          <Route path="notifications" element={wrapSettingsPage(<NotificationsTabbedPanel />)} />
          {/* Real device-pairing panel (replaces the old "Coming Soon" stub). */}
          <Route path="devices" element={wrapSettingsPage(<DevicesPanel />)} />

          {/* ── Assistant ───────────────────────────────────────────── */}
          <Route
            path="llm"
            element={wrapSettingsPage(<AIPanel />, { maxWidthClass: 'max-w-4xl' })}
          />
          <Route path="embeddings" element={wrapSettingsPage(<EmbeddingsPanel />)} />
          <Route
            path="usage"
            element={wrapSettingsPage(<UsagePanel />, { maxWidthClass: 'max-w-4xl' })}
          />
          <Route path="voice" element={wrapSettingsPage(<VoicePanel />)} />
          <Route path="personality" element={wrapSettingsPage(<PersonalityPanel />)} />
          <Route path="agents" element={wrapSettingsPage(<AgentsPanel />)} />
          <Route path="agents/new" element={wrapSettingsPage(<AgentEditorPage />)} />
          <Route path="agents/edit/:id" element={wrapSettingsPage(<AgentEditorPage />)} />
          <Route path="agent-access" element={wrapSettingsPage(<AgentAccessPanel />)} />
          <Route path="activity-level" element={wrapSettingsPage(<AgentActivityPanel />)} />
          <Route path="sandbox-settings" element={wrapSettingsPage(<SandboxSettingsPanel />)} />
          <Route path="approval-history" element={wrapSettingsPage(<ApprovalHistoryPanel />)} />

          {/* ── Data ────────────────────────────────────────────────── */}
          <Route
            path="memory-sync"
            element={wrapSettingsPage(<MemorySyncPanel />, { maxWidthClass: 'max-w-4xl' })}
          />
          <Route path="wallet-balances" element={wrapSettingsPage(<WalletBalancesPanel />)} />
          <Route path="recovery-phrase" element={wrapSettingsPage(<RecoveryPhrasePanel />)} />

          {/* ── Connections ─────────────────────────────────────────── */}
          <Route
            path="integrations"
            element={wrapSettingsPage(<IntegrationsPanel />, { maxWidthClass: 'max-w-4xl' })}
          />
          <Route
            path="screen-intelligence"
            element={wrapSettingsPage(<ScreenIntelligencePanel />)}
          />
          <Route path="tools" element={wrapSettingsPage(<ToolsPanel />)} />
          <Route path="companion" element={wrapSettingsPage(<CompanionPanel />)} />
          <Route path="autocomplete" element={wrapSettingsPage(<AutocompletePanel />)} />

          {/* ── System ──────────────────────────────────────────────── */}
          <Route path="developer-options" element={wrapSettingsPage(<DeveloperOptionsPanel />)} />
          <Route path="about" element={wrapSettingsPage(<AboutPanel />)} />

          {/* ── Developer & Diagnostics leaf panels ─────────────────── */}
          <Route
            path="tool-policy-diagnostics"
            element={wrapSettingsPage(<ToolPolicyDiagnosticsPanel />)}
          />
          <Route path="mcp-server" element={wrapSettingsPage(<McpServerPanel />)} />
          <Route path="search" element={wrapSettingsPage(<SearchPanel />)} />
          <Route path="agent-chat" element={wrapSettingsPage(<AgentChatPanel />)} />
          <Route path="cron-jobs" element={wrapSettingsPage(<CronJobsPanel />)} />
          <Route path="tasks" element={wrapSettingsPage(<TasksPanel />)} />
          <Route
            path="automations"
            element={wrapSettingsPage(<WorkflowsTab />, { maxWidthClass: 'max-w-4xl' })}
          />
          <Route path="dev-workflow" element={wrapSettingsPage(<DevWorkflowPanel />)} />
          <Route path="skills-runner" element={wrapSettingsPage(<WorkflowRunnerPanel />)} />
          <Route
            path="screen-awareness-debug"
            element={wrapSettingsPage(<ScreenAwarenessDebugPanel />)}
          />
          <Route path="autocomplete-debug" element={wrapSettingsPage(<AutocompleteDebugPanel />)} />
          <Route path="voice-debug" element={wrapSettingsPage(<VoiceDebugPanel />)} />
          <Route path="local-model-debug" element={wrapSettingsPage(<LocalModelDebugPanel />)} />
          <Route path="webhooks-debug" element={wrapSettingsPage(<WebhooksDebugPanel />)} />
          <Route path="event-log" element={wrapSettingsPage(<EventLogPanel />)} />
          <Route
            path="model-health"
            element={wrapSettingsPage(<ModelHealthPanel />, { maxWidthClass: 'max-w-4xl' })}
          />
          <Route path="memory-data" element={wrapSettingsPage(<MemoryDataPanel />)} />
          <Route path="memory-debug" element={wrapSettingsPage(<MemoryDebugPanel />)} />
          <Route
            path="analysis-views"
            element={wrapSettingsPage(<AnalysisViewsPanel />, { maxWidthClass: 'max-w-4xl' })}
          />
          <Route
            path="intelligence"
            element={wrapSettingsPage(<Intelligence />, { maxWidthClass: 'max-w-4xl' })}
          />
          <Route path="composio-triggers" element={wrapSettingsPage(<ComposioTriagePanel />)} />
          <Route path="permissions" element={wrapSettingsPage(<PermissionsPanel />)} />

          {/* ── Legacy slugs → redirects (deep-link compatibility) ──── */}
          {/* Old hub pages */}
          <Route path="ai" element={<Navigate to="/settings/llm" replace />} />
          <Route path="agents-settings" element={<Navigate to="/settings/agents" replace />} />
          <Route
            path="features"
            element={<Navigate to="/settings/screen-intelligence" replace />}
          />
          <Route path="crypto" element={<Navigate to="/settings/wallet-balances" replace />} />
          <Route
            path="notifications-hub"
            element={<Navigate to="/settings/notifications" replace />}
          />
          <Route path="composio" element={<Navigate to="/settings/integrations" replace />} />
          {/* Merged Usage & Limits page */}
          <Route path="heartbeat" element={<Navigate to="/settings/usage#background" replace />} />
          <Route
            path="ledger-usage"
            element={<Navigate to="/settings/usage#background" replace />}
          />
          <Route path="cost-dashboard" element={<Navigate to="/settings/usage" replace />} />
          {/* Autonomy rate-limit lives inside Agent access now */}
          <Route path="autonomy" element={<Navigate to="/settings/agent-access" replace />} />
          {/* Merged Personality & Face page */}
          <Route path="mascot" element={<Navigate to="/settings/personality#face" replace />} />
          <Route path="persona" element={<Navigate to="/settings/personality" replace />} />
          {/* Merged Integrations page */}
          <Route path="task-sources" element={<Navigate to="/settings/integrations" replace />} />
          <Route
            path="composio-routing"
            element={<Navigate to="/settings/integrations#composio" replace />}
          />
          <Route
            path="webhooks-triggers"
            element={<Navigate to="/settings/integrations#webhooks" replace />}
          />
          {/* Notification routing tab */}
          <Route
            path="notification-routing"
            element={<Navigate to="/settings/notifications#routing" replace />}
          />
          {/* Fallback */}
          <Route path="*" element={<Navigate to="/settings" replace />} />
        </Route>
      </Routes>
    </div>
  );
};

export default Settings;
