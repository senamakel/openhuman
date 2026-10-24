import { useEffect, useMemo, useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import { callCoreRpc } from '../../../services/coreRpcClient';
import Card from '../../ui/Card';
import { TileGrid } from '../../ui/TileGrid';
import { SettingsStatusLine } from '../controls';
import SettingsPanel from '../layout/SettingsPanel';

type ToolPolicyDiagnostics = {
  total_tools: number;
  enabled_tools: number;
  mcp_stdio_tools: number;
  json_rpc_tools: number;
  possible_write_surfaces: string[];
  policy_surfaces: string[];
  posture: {
    autonomy_level: string;
    workspace_only: boolean;
    max_actions_per_hour: number;
    require_approval_for_medium_risk: boolean;
    block_high_risk_commands: boolean;
  };
  mcp_allowlists: {
    enabled: boolean;
    server_count: number;
    enabled_server_count: number;
    servers: {
      name: string;
      enabled: boolean;
      allowed_tools_count: number;
      disallowed_tools_count: number;
      has_allowlist: boolean;
      has_denylist: boolean;
    }[];
  };
  mcp_write_audit: { enabled: boolean; recent_rows: number | null; last_error: string | null };
  recent_denials: {
    timestamp_ms: number;
    tool_name: string;
    policy: string;
    action: string;
    reason: string;
  }[];
};

type ModulePolicyInfo = {
  module?: {
    status: 'active' | 'unavailable' | 'not_built';
    generation?: number;
    enabled?: boolean;
    allowed_commands?: string[];
    forbidden_paths?: string[];
    capabilities?: string[];
  };
};

const ToolPolicyDiagnosticsPanel = () => {
  const { t } = useT();

  const [status, setStatus] = useState<
    | { kind: 'loading' }
    | { kind: 'ready'; diagnostics: ToolPolicyDiagnostics }
    | { kind: 'error'; message: string }
  >({ kind: 'loading' });
  const [moduleStatus, setModuleStatus] = useState<
    { kind: 'loading' } | { kind: 'ready'; info: ModulePolicyInfo } | { kind: 'error' }
  >({ kind: 'loading' });

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const diagnostics = await callCoreRpc<ToolPolicyDiagnostics>({
          method: 'openhuman.tool_registry_diagnostics',
          params: {},
          timeoutMs: 10_000,
        });
        if (cancelled) return;
        setStatus({ kind: 'ready', diagnostics });
      } catch (err) {
        if (cancelled) return;
        setStatus({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
      }
    })();
    (async () => {
      try {
        const info = await callCoreRpc<ModulePolicyInfo>({
          method: 'openhuman.security_policy_info',
          params: {},
          timeoutMs: 10_000,
        });
        if (cancelled) return;
        setModuleStatus({ kind: 'ready', info });
      } catch {
        if (cancelled) return;
        setModuleStatus({ kind: 'error' });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const body = useMemo(() => {
    if (status.kind === 'loading') {
      return (
        <div className="px-4 py-3 text-sm text-content-muted">
          {t('devOptions.toolPolicyDiagnostics.loading')}
        </div>
      );
    }
    if (status.kind === 'error') {
      return (
        <div className="px-4 py-3">
          <div className="text-sm font-semibold text-content mb-1">
            {t('devOptions.toolPolicyDiagnostics.unavailable')}
          </div>
          <SettingsStatusLine saving={false} error={status.message} savingLabel="" />
        </div>
      );
    }

    const d = status.diagnostics;
    const recentRows =
      d.mcp_write_audit.recent_rows === null ? '—' : String(d.mcp_write_audit.recent_rows);

    const T = 'devOptions.toolPolicyDiagnostics';
    const activeModule = moduleStatus.kind === 'ready' ? moduleStatus.info.module : undefined;
    const moduleValues =
      activeModule?.status === 'active'
        ? [
            [t(`${T}.module.generation`), String(activeModule.generation ?? '—')],
            [t(`${T}.module.enabled`), String(activeModule.enabled ?? '—')],
            [t(`${T}.module.allowedCommands`), activeModule.allowed_commands?.join(', ') || '—'],
            [t(`${T}.module.forbiddenPaths`), activeModule.forbidden_paths?.join(', ') || '—'],
            [t(`${T}.module.capabilities`), activeModule.capabilities?.join(', ') || '—'],
          ]
        : [];
    const stats: [string, number][] = [
      [t(`${T}.inventory.totalTools`), d.total_tools],
      [t(`${T}.inventory.enabledTools`), d.enabled_tools],
      [t(`${T}.inventory.mcpStdioTools`), d.mcp_stdio_tools],
      [t(`${T}.inventory.jsonRpcTools`), d.json_rpc_tools],
    ];
    const posture: [string, string, boolean][] = [
      [t(`${T}.posture.autonomy`), d.posture.autonomy_level, true],
      [t(`${T}.posture.workspaceOnly`), String(d.posture.workspace_only), false],
      [t(`${T}.posture.maxActionsPerHour`), String(d.posture.max_actions_per_hour), true],
      [
        t(`${T}.posture.approvalMediumRisk`),
        String(d.posture.require_approval_for_medium_risk),
        false,
      ],
      [t(`${T}.posture.blockHighRisk`), String(d.posture.block_high_risk_commands), false],
    ];

    // Short status cards: an inventory stat strip, then the detail cards in a
    // two-column grid, with the (possibly long) blocked-call list full width.
    return (
      <div className="space-y-4">
        <Card title={t(`${T}.module.title`)} divided={false}>
          {moduleStatus.kind === 'loading' ? (
            <p className="p-4 text-xs text-content-muted">{t(`${T}.module.loading`)}</p>
          ) : moduleStatus.kind === 'error' ||
            !activeModule ||
            activeModule.status === 'unavailable' ? (
            <p className="p-4 text-xs text-content-muted">{t(`${T}.module.unavailable`)}</p>
          ) : activeModule.status === 'not_built' ? (
            <p className="p-4 text-xs text-content-muted">{t(`${T}.module.notBuilt`)}</p>
          ) : (
            <dl className="divide-y divide-line-subtle pb-1 pt-2 text-xs">
              {moduleValues.map(([label, value]) => (
                <div key={label} className="flex items-start justify-between gap-3 px-4 py-2">
                  <dt className="shrink-0 text-content-muted">{label}</dt>
                  <dd className="wrap-break-word text-right font-mono text-content">{value}</dd>
                </div>
              ))}
            </dl>
          )}
        </Card>
        <Card title={t(`${T}.inventory.title`)} divided={false}>
          <dl className="grid grid-cols-2 gap-3 p-4 lg:grid-cols-4">
            {stats.map(([label, value]) => (
              <div
                key={label}
                className="rounded-lg border border-line bg-surface-muted/40 px-3 py-2.5">
                <dt className="text-xs text-content-muted">{label}</dt>
                <dd className="mt-0.5 font-mono text-lg font-semibold text-content">{value}</dd>
              </div>
            ))}
          </dl>
        </Card>

        <TileGrid columns={2}>
          <Card title={t(`${T}.posture.title`)} className="h-full" divided={false}>
            <dl className="divide-y divide-line-subtle pb-1 pt-2 text-xs">
              {posture.map(([label, value, mono]) => (
                <div key={label} className="flex items-center justify-between gap-3 px-4 py-2">
                  <dt className="text-content-muted">{label}</dt>
                  <dd className={mono ? 'font-mono text-content' : 'text-content'}>{value}</dd>
                </div>
              ))}
            </dl>
          </Card>

          <Card title={t(`${T}.mcpAllowlists.title`)} className="h-full" divided={false}>
            <div className="space-y-2 p-4 text-xs">
              <p className="text-content-muted">
                {t(`${T}.mcpAllowlists.summary`)
                  .replace('{enabled}', String(d.mcp_allowlists.enabled))
                  .replace('{enabledCount}', String(d.mcp_allowlists.enabled_server_count))
                  .replace('{totalCount}', String(d.mcp_allowlists.server_count))}
              </p>
              {d.mcp_allowlists.servers.length > 0 && (
                <ul className="space-y-1">
                  {d.mcp_allowlists.servers.slice(0, 10).map(s => (
                    <li key={s.name} className="flex items-center justify-between gap-3">
                      <span className="truncate font-mono text-content" title={s.name}>
                        {s.name || t(`${T}.mcpAllowlists.unnamed`)}
                      </span>
                      <span className="font-mono text-content-muted">
                        {t(`${T}.mcpAllowlists.allowDeny`)
                          .replace('{allowCount}', String(s.allowed_tools_count))
                          .replace('{denyCount}', String(s.disallowed_tools_count))}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </Card>

          <Card title={t(`${T}.mcpWriteAudit.title`)} className="h-full" divided={false}>
            <div className="space-y-2 p-4 text-xs">
              <p className="text-content-muted">
                {t(`${T}.mcpWriteAudit.summary`)
                  .replace('{enabled}', String(d.mcp_write_audit.enabled))
                  .replace('{recentRows}', recentRows)}
              </p>
              {d.mcp_write_audit.last_error && (
                <p className="wrap-break-word font-mono text-coral-700 dark:text-coral-200">
                  {d.mcp_write_audit.last_error}
                </p>
              )}
            </div>
          </Card>

          <Card title={t(`${T}.redactedSurfaces.title`)} className="h-full" divided={false}>
            <p className="p-4 text-xs text-content-muted">
              {t(`${T}.redactedSurfaces.summary`)
                .replace('{writeCount}', String(d.possible_write_surfaces.length))
                .replace('{policyCount}', String(d.policy_surfaces.length))}
            </p>
          </Card>
        </TileGrid>

        <Card title={t(`${T}.recentBlocked.title`)}>
          {d.recent_denials.length === 0 ? (
            <p className="p-4 text-xs text-content-muted">{t(`${T}.recentBlocked.empty`)}</p>
          ) : (
            d.recent_denials.slice(0, 10).map(entry => (
              <div
                key={`${entry.timestamp_ms}:${entry.tool_name}`}
                className="flex flex-col gap-0.5 px-4 py-2.5 text-xs">
                <div className="flex items-center justify-between gap-3">
                  <span className="truncate font-mono text-content" title={entry.tool_name}>
                    {entry.tool_name}
                  </span>
                  <span className="font-mono text-content-muted">
                    {entry.policy}:{entry.action}
                  </span>
                </div>
                <p className="wrap-break-word text-content-muted">{entry.reason}</p>
              </div>
            ))
          )}
        </Card>
      </div>
    );
  }, [moduleStatus, status, t]);

  return (
    <SettingsPanel description={t('devOptions.toolPolicyDiagnosticsDesc')}>{body}</SettingsPanel>
  );
};

export default ToolPolicyDiagnosticsPanel;
