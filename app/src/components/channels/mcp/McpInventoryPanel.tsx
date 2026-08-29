/**
 * Sharable MCP Inventory — top-level modal hosting Export / Import tabs.
 *
 * The parent (`McpServersTab`) holds the open/close state and the
 * current `servers` array; this component owns the tab navigation and
 * dispatches the install-via-existing-dialog flow back upward.
 *
 * Why a single modal with tabs (rather than two separate modals):
 *   - The user often flips between "let me see what I have" (Export)
 *     and "let me apply what someone sent" (Import) in the same
 *     session — tabbing is faster than re-opening.
 *   - The dialog focus contract (`role="dialog" aria-modal`) is
 *     simpler to maintain on a single mount.
 *
 * Esc closes the modal; backdrop mousedown closes; clicks inside the
 * card do not.
 */
import { useId, useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import { ModalShell } from '../../ui/ModalShell';
import { TabsContent, TabsList, TabsRoot, TabsTrigger } from '../../ui/Tabs';
import McpInventoryExportTab from './McpInventoryExportTab';
import McpInventoryImportTab from './McpInventoryImportTab';
import type { InstalledServer } from './types';

interface McpInventoryPanelProps {
  /** Current installed servers — drives the Export tab and the
   *  "already installed" detection in the Import tab. */
  servers: InstalledServer[];
  /**
   * Called when the user clicks "Install" on an entry in the Import
   * preview. Parent wires this to its existing install-dialog flow
   * (`setRightPane({ mode: 'install', qualifiedName, prefillEnv })`)
   * so the proven InstallDialog handles env-value collection — we
   * never re-implement that critical surface here.
   */
  onInstallServer: (qualifiedName: string, prefillEnv: Record<string, string>) => void;
  onClose: () => void;
}

type Tab = 'export' | 'import';

const McpInventoryPanel = ({ servers, onInstallServer, onClose }: McpInventoryPanelProps) => {
  const { t } = useT();
  const [tab, setTab] = useState<Tab>('export');
  const titleId = useId();

  return (
    <ModalShell
      onClose={onClose}
      titleId={titleId}
      title={t('mcp.inventory.title')}
      subtitle={t('mcp.inventory.subtitle')}
      maxWidthClassName="max-w-3xl"
      contentClassName="max-h-full overflow-y-auto p-5">
      <TabsRoot value={tab} onValueChange={value => setTab(value as Tab)}>
        <TabsList
          aria-label={t('mcp.inventory.tablistAria')}
          className="mb-4 justify-start gap-1 border-b border-line">
          <TabsTrigger
            value="export"
            className="-mb-px rounded-none border-b-2 border-transparent px-3 py-1.5 data-[state=active]:border-primary-500 data-[state=active]:bg-transparent data-[state=active]:text-primary-600 dark:data-[state=active]:text-primary-300">
            {t('mcp.inventory.tab.export')}
          </TabsTrigger>
          <TabsTrigger
            value="import"
            className="-mb-px rounded-none border-b-2 border-transparent px-3 py-1.5 data-[state=active]:border-primary-500 data-[state=active]:bg-transparent data-[state=active]:text-primary-600 dark:data-[state=active]:text-primary-300">
            {t('mcp.inventory.tab.import')}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="export">
          <McpInventoryExportTab servers={servers} />
        </TabsContent>
        <TabsContent value="import">
          <McpInventoryImportTab
            installedServers={servers}
            onInstallServer={(qualifiedName, prefillEnv) => {
              // The parent's install flow lives outside this modal — close
              // the inventory panel so the InstallDialog has room to render
              // in the main right pane.
              onInstallServer(qualifiedName, prefillEnv);
              onClose();
            }}
          />
        </TabsContent>
      </TabsRoot>
    </ModalShell>
  );
};

export default McpInventoryPanel;
