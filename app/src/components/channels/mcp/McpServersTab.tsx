/**
 * Top-level MCP Servers tab.
 *
 * Single-pane layout: installed servers at the top (collapsible), catalog
 * browser below. Selecting an installed server or clicking "Install" on a
 * catalog card replaces the view with a detail/install pane + back button.
 */
import debug from 'debug';
import { useCallback, useEffect, useRef, useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import { mcpClientsApi } from '../../../services/api/mcpClientsApi';
import InstallDialog from './InstallDialog';
import InstalledServerDetail from './InstalledServerDetail';
import InstalledServerList from './InstalledServerList';
import McpCatalogBrowser from './McpCatalogBrowser';
import McpConnectionHealthToolbar from './McpConnectionHealthToolbar';
import McpInventoryPanel from './McpInventoryPanel';
import McpServerSearch from './McpServerSearch';
import type { ConnStatus, InstalledServer } from './types';

const log = debug('mcp-clients:tab');
const POLL_INTERVAL_MS = 5_000;

type View =
  | { mode: 'home' }
  | { mode: 'detail'; serverId: string }
  | { mode: 'install'; qualifiedName: string; prefillEnv?: Record<string, string> };

const McpServersTab = () => {
  const { t } = useT();
  const [servers, setServers] = useState<InstalledServer[]>([]);
  const [statuses, setStatuses] = useState<ConnStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [view, setView] = useState<View>({ mode: 'home' });
  const [searchFilter, setSearchFilter] = useState('');
  const [inventoryOpen, setInventoryOpen] = useState(false);
  const [installedExpanded, setInstalledExpanded] = useState(true);
  const pollTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const loadInstalled = useCallback(async () => {
    log('loading installed servers');
    try {
      const installed = await mcpClientsApi.installedList();
      setServers(Array.isArray(installed) ? installed : []);
      setLoadError(null);
      log('loaded %d installed servers', installed.length);
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to load installed servers';
      log('load error: %s', msg);
      setLoadError(msg);
    }
  }, []);

  const fetchStatuses = useCallback(async () => {
    log('polling statuses');
    try {
      const sv = await mcpClientsApi.status();
      setStatuses(Array.isArray(sv) ? sv : []);
    } catch (err) {
      log('status poll error: %o', err);
    }
  }, []);

  useEffect(() => {
    Promise.all([loadInstalled(), fetchStatuses()]).finally(() => setLoading(false));
  }, [loadInstalled, fetchStatuses]);

  useEffect(() => {
    const hasConnected = statuses.some(s => s.status === 'connected');
    if (!hasConnected) {
      if (pollTimerRef.current) {
        clearTimeout(pollTimerRef.current);
        pollTimerRef.current = null;
      }
      return;
    }

    const schedule = () => {
      pollTimerRef.current = setTimeout(async () => {
        await fetchStatuses();
        schedule();
      }, POLL_INTERVAL_MS);
    };
    schedule();

    return () => {
      if (pollTimerRef.current) {
        clearTimeout(pollTimerRef.current);
        pollTimerRef.current = null;
      }
    };
  }, [statuses, fetchStatuses]);

  const handleSelectServer = useCallback((serverId: string) => {
    log('selected server_id=%s', serverId);
    setView({ mode: 'detail', serverId });
  }, []);

  const handleSelectInstall = useCallback((qualifiedName: string) => {
    log('opening install dialog for %s', qualifiedName);
    setView({ mode: 'install', qualifiedName });
  }, []);

  const handleInstallSuccess = useCallback(
    async (server: InstalledServer) => {
      log('install success server_id=%s, refreshing list', server.server_id);
      await loadInstalled();
      await fetchStatuses();
      setView({ mode: 'detail', serverId: server.server_id });
    },
    [loadInstalled, fetchStatuses]
  );

  const handleUninstalled = useCallback(
    async (serverId: string) => {
      log('uninstalled server_id=%s', serverId);
      await loadInstalled();
      await fetchStatuses();
      setView({ mode: 'home' });
    },
    [loadInstalled, fetchStatuses]
  );

  const handleEnabledChange = useCallback(
    async (_serverId: string, _enabled: boolean) => {
      log('enabled_change server_id=%s enabled=%s', _serverId, _enabled);
      await loadInstalled();
      await fetchStatuses();
    },
    [loadInstalled, fetchStatuses]
  );

  const reportBulkFailures = useCallback(
    (results: PromiseSettledResult<unknown>[], total: number) => {
      const failed = results.filter(r => r.status === 'rejected').length;
      if (failed > 0) {
        log('bulk op partial failure: %d/%d failed', failed, total);
        throw new Error(
          t('mcp.health.bulkPartialFailure')
            .replace('{failed}', String(failed))
            .replace('{total}', String(total))
        );
      }
    },
    [t]
  );

  const handleBulkReconnect = useCallback(
    async (serverIds: string[]) => {
      log('bulk reconnect ids=%o', serverIds);
      const results = await Promise.allSettled(serverIds.map(id => mcpClientsApi.connect(id)));
      await fetchStatuses();
      reportBulkFailures(results, serverIds.length);
    },
    [fetchStatuses, reportBulkFailures]
  );

  const handleBulkDisconnect = useCallback(
    async (serverIds: string[]) => {
      log('bulk disconnect ids=%o', serverIds);
      const results = await Promise.allSettled(serverIds.map(id => mcpClientsApi.disconnect(id)));
      await fetchStatuses();
      reportBulkFailures(results, serverIds.length);
    },
    [fetchStatuses, reportBulkFailures]
  );

  const selectedServer =
    view.mode === 'detail' ? (servers.find(s => s.server_id === view.serverId) ?? null) : null;
  const selectedConnStatus =
    view.mode === 'detail' ? statuses.find(s => s.server_id === view.serverId) : undefined;

  if (loading) {
    return (
      <div className="py-10 text-center text-sm text-stone-400 dark:text-neutral-500">
        {t('mcp.tab.loading')}
      </div>
    );
  }

  // Detail or install view — full-pane with back button
  if (view.mode === 'detail' && selectedServer) {
    return (
      <div className="space-y-3">
        <button
          type="button"
          onClick={() => setView({ mode: 'home' })}
          className="inline-flex items-center gap-1.5 text-xs font-medium text-stone-500 dark:text-neutral-400 hover:text-stone-700 dark:hover:text-neutral-200 transition-colors">
          <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M15 19l-7-7 7-7" />
          </svg>
          {t('mcp.install.back')}
        </button>
        <InstalledServerDetail
          server={selectedServer}
          connStatus={selectedConnStatus}
          onUninstalled={serverId => void handleUninstalled(serverId)}
          onEnabledChange={(serverId, enabled) => void handleEnabledChange(serverId, enabled)}
        />
      </div>
    );
  }

  if (view.mode === 'install') {
    return (
      <div className="space-y-3">
        <button
          type="button"
          onClick={() => setView({ mode: 'home' })}
          className="inline-flex items-center gap-1.5 text-xs font-medium text-stone-500 dark:text-neutral-400 hover:text-stone-700 dark:hover:text-neutral-200 transition-colors">
          <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M15 19l-7-7 7-7" />
          </svg>
          {t('mcp.install.back')}
        </button>
        <InstallDialog
          qualifiedName={view.qualifiedName}
          prefillEnv={view.prefillEnv}
          onSuccess={server => void handleInstallSuccess(server)}
          onCancel={() => setView({ mode: 'home' })}
        />
      </div>
    );
  }

  // Home view — installed servers + catalog
  return (
    <div className="space-y-4">
      {/* Alpha banner + inventory */}
      <div className="flex items-center gap-2">
        <div
          role="status"
          className="flex-1 flex items-start gap-2 rounded-lg border border-amber-200 dark:border-amber-500/30 bg-amber-50 dark:bg-amber-500/10 px-3 py-2 text-xs text-amber-800 dark:text-amber-200">
          <span className="inline-flex items-center px-1.5 py-0.5 rounded text-[9px] font-semibold uppercase tracking-wider bg-amber-200/70 dark:bg-amber-500/30 text-amber-900 dark:text-amber-100 shrink-0 mt-0.5">
            {t('mcp.alphaBadge')}
          </span>
          <span className="leading-relaxed">{t('mcp.alphaBannerText')}</span>
        </div>
        <button
          type="button"
          onClick={() => setInventoryOpen(true)}
          aria-label={t('mcp.inventory.openAria')}
          className="shrink-0 rounded-lg border border-stone-200 dark:border-neutral-700 px-3 py-2 text-xs font-medium text-stone-700 dark:text-neutral-200 hover:bg-stone-50 dark:hover:bg-neutral-800">
          {t('mcp.inventory.openButton')}
        </button>
      </div>

      {loadError && (
        <div className="rounded-lg border border-coral-200 dark:border-coral-500/30 bg-coral-50 dark:bg-coral-500/10 px-3 py-2 text-xs text-coral-700 dark:text-coral-300">
          {loadError}
        </div>
      )}

      {/* Installed servers section */}
      {servers.length > 0 && (
        <div className="rounded-lg border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 overflow-hidden">
          <button
            type="button"
            onClick={() => setInstalledExpanded(prev => !prev)}
            className="w-full flex items-center justify-between px-4 py-3 hover:bg-stone-50 dark:hover:bg-neutral-800/60 transition-colors">
            <div className="flex items-center gap-2">
              <h3 className="text-xs font-semibold text-stone-700 dark:text-neutral-300 uppercase tracking-wide">
                {t('mcp.installed.title')}
              </h3>
              <span className="text-[11px] text-stone-400 dark:text-neutral-500">
                ({servers.length})
              </span>
            </div>
            <svg
              className={`w-4 h-4 text-stone-400 dark:text-neutral-500 transition-transform ${installedExpanded ? 'rotate-180' : ''}`}
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
            </svg>
          </button>

          {installedExpanded && (
            <div className="border-t border-stone-100 dark:border-neutral-800 px-4 pb-3">
              <McpConnectionHealthToolbar
                statuses={statuses}
                onReconnect={handleBulkReconnect}
                onDisconnect={handleBulkDisconnect}
              />
              {servers.length > 3 && (
                <div className="mb-2">
                  <McpServerSearch value={searchFilter} onChange={setSearchFilter} />
                </div>
              )}
              <InstalledServerList
                servers={servers}
                statuses={statuses}
                selectedId={null}
                onSelect={handleSelectServer}
                onBrowseCatalog={() => {}}
                filter={searchFilter}
              />
            </div>
          )}
        </div>
      )}

      {/* Catalog browser — always visible on home */}
      <div>
        <h3 className="text-xs font-semibold text-stone-700 dark:text-neutral-300 uppercase tracking-wide mb-3">
          {t('mcp.installed.browseCatalog')}
        </h3>
        <McpCatalogBrowser onSelectInstall={handleSelectInstall} />
      </div>

      {inventoryOpen && (
        <McpInventoryPanel
          servers={servers}
          onInstallServer={(qualifiedName, prefillEnv) => {
            setView({ mode: 'install', qualifiedName, prefillEnv });
          }}
          onClose={() => setInventoryOpen(false)}
        />
      )}
    </div>
  );
};

export default McpServersTab;
