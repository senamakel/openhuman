/**
 * OverviewPanel — an interactive graph of the agent / sub-agent system, reusing
 * the Brain's {@link MemoryGraph} (drag / pan / zoom, WebGL with SVG fallback).
 *
 * Three tiers fan out from a central **agent core** hub:
 *   - core (the synthetic root, labelled for OpenHuman)
 *   - **instances** — the peer contacts you coordinate with (`source` nodes)
 *   - **sub-agents** — the sessions running under each instance (`chunk` nodes),
 *     linked to their instance by explicit edges.
 *
 * Data comes from {@link useContactSessions} (live, socket-refreshed), so the
 * graph reflects the same instances + sub-agents as the sidebar rail.
 */
import { useMemo } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import type { SessionSummary } from '../../lib/orchestration/orchestrationClient';
import { useContactSessions } from '../../lib/orchestration/useOrchestrationSessions';
import type { GraphEdge, GraphNode } from '../../utils/tauriCommands';
import { MemoryGraph } from '../intelligence/MemoryGraph';

function shortAddress(address: string): string {
  return address.length <= 14 ? address : `${address.slice(0, 6)}…${address.slice(-4)}`;
}

function sessionLabel(session: SessionSummary): string {
  return session.label?.trim() || session.sessionId;
}

export default function OverviewPanel() {
  const { t } = useT();
  const { byContact } = useContactSessions();

  const { nodes, edges } = useMemo(() => {
    const nodes: GraphNode[] = [];
    const edges: GraphEdge[] = [];
    for (const [address, sessions] of byContact.entries()) {
      const instanceId = `inst:${address}`;
      // `source` nodes auto-fan-out from the synthetic core hub (WebGL path).
      nodes.push({ kind: 'source', id: instanceId, label: shortAddress(address) });
      for (const session of sessions) {
        const sessionNodeId = `sess:${session.sessionId}`;
        nodes.push({ kind: 'chunk', id: sessionNodeId, label: sessionLabel(session) });
        edges.push({ from: sessionNodeId, to: instanceId });
      }
    }
    return { nodes, edges };
  }, [byContact]);

  return (
    // Full-screen: the graph fills the whole pane (no card gutter) and fits the
    // whole agent/sub-agent cloud tightly to the viewport.
    <div className="h-full p-2" data-testid="orch-overview">
      <MemoryGraph
        nodes={nodes}
        edges={edges}
        mode="contacts"
        rootLabel={t('orchPage.overview.core')}
        emptyHint={t('orchPage.overview.empty')}
        fill
        fitToBounds
      />
    </div>
  );
}
