/**
 * EditableFlowCanvas — the mutable Workflow Canvas (issue B5b.2 / Phase 3a).
 * Wraps `@xyflow/react`'s `<ReactFlow>` in *controlled* mode: node/edge state
 * is lifted into `useNodesState`/`useEdgesState` seeded once from the incoming
 * graph, so drags, connections, additions, and deletions mutate local state
 * rather than the read-only viewer's static props.
 *
 * What it wires on top of the read-only `FlowCanvas`:
 *  - **drag / move** — `nodesDraggable` on; `onNodesChange` persists positions.
 *  - **connect** — `onConnect` is port-aware: it accepts a new edge only when
 *    {@link isValidFlowConnection} approves it (reusing the canvas's derived
 *    input/output ports), and rejects self-loops, unknown handles, and dupes.
 *  - **delete** — Backspace/Delete removes the selection (React Flow default),
 *    plus an explicit "Delete selected" toolbar button as a discoverable
 *    affordance; deleting a node also drops its incident edges.
 *  - **add** — a {@link NodePalette} inserts any of the 12 node kinds by click
 *    (default cascade position) or drag-drop (under the cursor).
 *  - **save** — a "Save" button serializes the live canvas back to a
 *    `WorkflowGraph` via {@link xyflowToWorkflowGraph} and hands it to `onSave`.
 *    The dirty-guard / persistence call lives one layer up (Phase 3d).
 */
import {
  addEdge,
  Background,
  BackgroundVariant,
  type Connection,
  Controls,
  MiniMap,
  ReactFlow,
  type ReactFlowInstance,
  useEdgesState,
  useNodesState,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import createDebug from 'debug';
import { memo, useCallback, useEffect, useRef, useState } from 'react';

import {
  createFlowNode,
  FLOW_NODE_TYPE,
  type FlowEdge,
  type FlowNode,
  isValidFlowConnection,
  type WorkflowGraphMeta,
  xyflowToWorkflowGraph,
} from '../../../lib/flows/graphAdapter';
import type { NodeKind, WorkflowGraph } from '../../../lib/flows/types';
import { useT } from '../../../lib/i18n/I18nContext';
import { type FlowConnection, listFlowConnections } from '../../../services/api/flowsApi';
import Button from '../../ui/Button';
import './flowCanvasStyles.css';
import FlowNodeComponent from './FlowNodeComponent';
import NodeConfigDrawer, { type NodeConfigPatch } from './nodeConfig/NodeConfigDrawer';
import NodePalette, { PALETTE_DND_MIME } from './NodePalette';

const log = createDebug('app:flows:canvas:edit');

const NODE_TYPES = { [FLOW_NODE_TYPE]: FlowNodeComponent };
const DELETE_KEYS = ['Backspace', 'Delete'];

/** Where a click-added palette node lands (canvas coords) before cascade. */
const CLICK_ADD_ORIGIN = { x: 80, y: 80 };
/** Per-click cascade so repeated palette clicks don't stack on one spot. */
const CLICK_ADD_STEP = 32;

export interface EditableFlowCanvasProps {
  nodes: FlowNode[];
  edges: FlowEdge[];
  /** Graph-level metadata xyflow doesn't carry, needed to re-serialize on save. */
  meta: WorkflowGraphMeta;
  /**
   * Called with the current canvas serialized to a `WorkflowGraph` when the
   * user clicks Save. The caller owns validation + the `flows_update` RPC
   * (Phase 3c/3d); this component only produces the graph.
   */
  onSave?: (graph: WorkflowGraph) => void;
  /** Fired when a drawn connection is rejected as invalid (for a toast in 3c). */
  onInvalidConnection?: (connection: Connection) => void;
}

function EditableFlowCanvas({
  nodes: initialNodes,
  edges: initialEdges,
  meta,
  onSave,
  onInvalidConnection,
}: EditableFlowCanvasProps) {
  const { t } = useT();
  const [nodes, setNodes, onNodesChange] = useNodesState<FlowNode>(initialNodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState<FlowEdge>(initialEdges);
  const rfRef = useRef<ReactFlowInstance<FlowNode, FlowEdge> | null>(null);
  const addCounter = useRef(0);
  const [selectionCount, setSelectionCount] = useState(0);
  // Id of the single selected node whose config the drawer edits (`null` when
  // zero or multiple nodes — or any edge — are selected).
  const [configNodeId, setConfigNodeId] = useState<string | null>(null);
  const [connections, setConnections] = useState<FlowConnection[]>([]);

  // Load the secret-free credential refs once for the node-config credential
  // picker (http_request / tool_call). Guarded: outside Tauri (or if the RPC
  // fails) the picker just shows its empty state rather than throwing.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const list = await listFlowConnections();
        if (cancelled) return;
        log('connections loaded: count=%d', list.length);
        setConnections(list);
      } catch (err) {
        if (cancelled) return;
        log('connections load failed (non-fatal): %o', err);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const nextNodeId = useCallback((kind: NodeKind): string => {
    // Prefix keeps palette-added ids from ever colliding with loaded graph ids
    // (which are arbitrary backend strings); the counter keeps them unique
    // within a session even for same-kind, same-millisecond clicks.
    return `new-${kind}-${addCounter.current++}`;
  }, []);

  const onConnect = useCallback(
    (connection: Connection) => {
      if (!isValidFlowConnection(connection, nodes, edges)) {
        log('onConnect: rejected %o', connection);
        onInvalidConnection?.(connection);
        return;
      }
      log('onConnect: accepted %o', connection);
      setEdges(current => addEdge(connection, current));
    },
    [nodes, edges, setEdges, onInvalidConnection]
  );

  // Live drag feedback: React Flow calls this while dragging a new connection
  // and paints the target handle valid/invalid before the drop commits.
  const isValidConnection = useCallback(
    (connection: Connection | FlowEdge) =>
      isValidFlowConnection(connection as Connection, nodes, edges),
    [nodes, edges]
  );

  const addNode = useCallback(
    (kind: NodeKind, position: { x: number; y: number }) => {
      const id = nextNodeId(kind);
      const name = t(`flows.nodeKind.${kind}`, kind);
      const node = createFlowNode(kind, position, id, name);
      log('addNode: kind=%s id=%s at %o', kind, id, position);
      setNodes(current => [...current, node]);
    },
    [nextNodeId, setNodes, t]
  );

  const handlePaletteAdd = useCallback(
    (kind: NodeKind) => {
      const step = addCounter.current * CLICK_ADD_STEP;
      addNode(kind, { x: CLICK_ADD_ORIGIN.x + step, y: CLICK_ADD_ORIGIN.y + step });
    },
    [addNode]
  );

  const handleDrop = useCallback(
    (event: React.DragEvent) => {
      event.preventDefault();
      const kind = event.dataTransfer.getData(PALETTE_DND_MIME) as NodeKind;
      if (!kind) return;
      const instance = rfRef.current;
      const position = instance
        ? instance.screenToFlowPosition({ x: event.clientX, y: event.clientY })
        : { ...CLICK_ADD_ORIGIN };
      addNode(kind, position);
    },
    [addNode]
  );

  const handleDragOver = useCallback((event: React.DragEvent) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
  }, []);

  const handleDeleteSelected = useCallback(() => {
    const removedNodeIds = new Set(nodes.filter(n => n.selected).map(n => n.id));
    const removedEdgeIds = new Set(edges.filter(e => e.selected).map(e => e.id));
    if (removedNodeIds.size === 0 && removedEdgeIds.size === 0) return;
    log('deleteSelected: nodes=%d edges=%d', removedNodeIds.size, removedEdgeIds.size);
    setNodes(current => current.filter(n => !removedNodeIds.has(n.id)));
    // Drop explicitly-selected edges AND any edge left dangling by a removed node.
    setEdges(current =>
      current.filter(
        e =>
          !removedEdgeIds.has(e.id) &&
          !removedNodeIds.has(e.source) &&
          !removedNodeIds.has(e.target)
      )
    );
  }, [nodes, edges, setNodes, setEdges]);

  const handleSave = useCallback(() => {
    const graph = xyflowToWorkflowGraph(nodes, edges, meta);
    log('save: nodes=%d edges=%d', graph.nodes.length, graph.edges.length);
    onSave?.(graph);
  }, [nodes, edges, meta, onSave]);

  const onSelectionChange = useCallback(
    ({ nodes: selNodes, edges: selEdges }: { nodes: FlowNode[]; edges: FlowEdge[] }) => {
      setSelectionCount(selNodes.length + selEdges.length);
      // Open the config drawer only for an unambiguous single-node selection;
      // any edge in the selection, or 0/2+ nodes, closes it.
      const nextId = selEdges.length === 0 && selNodes.length === 1 ? selNodes[0].id : null;
      log(
        'selectionChange: nodes=%d edges=%d configNode=%s',
        selNodes.length,
        selEdges.length,
        nextId ?? 'none'
      );
      setConfigNodeId(nextId);
    },
    []
  );

  // Apply a name/config edit from the drawer to the live node state (controlled).
  const updateNode = useCallback(
    (nodeId: string, patch: NodeConfigPatch) => {
      log(
        'updateNode: id=%s name=%s config=%s',
        nodeId,
        patch.name ?? '(unchanged)',
        patch.config ? 'present' : '(unchanged)'
      );
      setNodes(current =>
        current.map(n =>
          n.id === nodeId
            ? {
                ...n,
                data: {
                  ...n.data,
                  ...(patch.name !== undefined ? { name: patch.name } : {}),
                  ...(patch.config !== undefined ? { config: patch.config } : {}),
                },
              }
            : n
        )
      );
    },
    [setNodes]
  );

  // Close the drawer AND clear the selection, so re-clicking the same node
  // re-fires `onSelectionChange` and reopens it.
  const handleCloseConfig = useCallback(() => {
    log('closeConfig: deselecting all nodes');
    setConfigNodeId(null);
    setNodes(current =>
      current.some(n => n.selected) ? current.map(n => ({ ...n, selected: false })) : current
    );
  }, [setNodes]);

  const configNode = configNodeId ? (nodes.find(n => n.id === configNodeId) ?? null) : null;

  return (
    <div
      className="flow-canvas relative h-full w-full"
      data-testid="flow-canvas"
      data-editable="true"
      onDrop={handleDrop}
      onDragOver={handleDragOver}>
      <NodePalette onAdd={handlePaletteAdd} />

      <div className="pointer-events-none absolute right-3 top-3 z-10 flex items-center gap-2">
        <Button
          type="button"
          variant="secondary"
          tone="danger"
          size="xs"
          className="pointer-events-auto"
          data-testid="flow-editor-delete"
          disabled={selectionCount === 0}
          onClick={handleDeleteSelected}>
          {t('flows.editor.deleteSelected')}
        </Button>
        {onSave && (
          <Button
            type="button"
            variant="primary"
            size="xs"
            className="pointer-events-auto"
            data-testid="flow-editor-save"
            onClick={handleSave}>
            {t('flows.editor.save')}
          </Button>
        )}
      </div>

      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={NODE_TYPES}
        onInit={instance => {
          rfRef.current = instance;
        }}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        isValidConnection={isValidConnection}
        onSelectionChange={onSelectionChange}
        deleteKeyCode={DELETE_KEYS}
        nodesDraggable
        nodesConnectable
        elementsSelectable
        fitView
        panOnScroll
        zoomOnScroll>
        <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
        <MiniMap pannable zoomable />
        <Controls showInteractive={false} />
      </ReactFlow>

      <NodeConfigDrawer
        node={configNode}
        onClose={handleCloseConfig}
        onChange={updateNode}
        connections={connections}
      />
    </div>
  );
}

export default memo(EditableFlowCanvas);
