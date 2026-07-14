import { useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import ChipTabs from '../../layout/ChipTabs';
import AgentChatPanel from './AgentChatPanel';
import AIPanel from './AIPanel';
import LocalModelDebugPanel from './LocalModelDebugPanel';

type LlmChip = 'api-keys' | 'local-model' | 'agent-chat';

/**
 * The Connections → LLM surface as a three-chip page:
 *   - **API keys** — the main AI provider / model configuration (AIPanel).
 *   - **Local Model Debug** — local runtime status + per-capability testers.
 *   - **Agent Chat Debug** — the raw agent-chat tester.
 *
 * Local Model Debug and Agent Chat used to be standalone Developer Options
 * pages; they're folded in here so everything LLM-related lives on one page.
 * The active chip is local UI state (not a route) — deep links land on the
 * API-keys chip.
 */
const LlmConnectionsPanel = () => {
  const { t } = useT();
  const [chip, setChip] = useState<LlmChip>('api-keys');

  return (
    <div className="flex h-full flex-col">
      <ChipTabs<LlmChip>
        ariaLabel={t('pages.settings.ai.llm')}
        testIdPrefix="llm-chip"
        value={chip}
        onChange={setChip}
        items={[
          { id: 'api-keys', label: t('pages.settings.ai.llm') },
          { id: 'local-model', label: t('settings.developerMenu.localModelDebug.title') },
          { id: 'agent-chat', label: t('settings.developerMenu.agentChat.title') },
        ]}
      />
      <div className="min-h-0 flex-1 overflow-y-auto">
        {chip === 'api-keys' && <AIPanel embedded />}
        {chip === 'local-model' && (
          <div className="p-4">
            <LocalModelDebugPanel embedded />
          </div>
        )}
        {chip === 'agent-chat' && (
          <div className="p-4">
            <AgentChatPanel embedded />
          </div>
        )}
      </div>
    </div>
  );
};

export default LlmConnectionsPanel;
