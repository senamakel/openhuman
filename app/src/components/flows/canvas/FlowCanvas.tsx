/**
 * FlowCanvas — the Workflow Canvas view. Two modes behind one entry point:
 *
 *  - **read-only** (default, issue B5b.1): renders a saved flow's
 *    `WorkflowGraph` (already converted to xyflow's shape by `graphAdapter.ts`)
 *    with a minimap, zoom/pan controls, and a dotted background. Every
 *    interaction that would mutate the graph is disabled.
 *  - **editable** (`editable`, issue B5b.2 / Phase 3a): delegates to
 *    {@link EditableFlowCanvas}, which lifts nodes/edges into controlled state
 *    and wires drag/connect/add/delete/save on top.
 *
 * The `editable` prop defaults to `false` so every existing read-only consumer
 * (the `/flows/:id` viewer) keeps its exact behavior — only the builder opts in.
 */
import { Background, BackgroundVariant, Controls, MiniMap, ReactFlow } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { memo, useMemo } from 'react';

import {
  FLOW_NODE_TYPE,
  type FlowEdge,
  type FlowNode,
  type WorkflowGraphMeta,
} from '../../../lib/flows/graphAdapter';
import type { WorkflowGraph } from '../../../lib/flows/types';
import EditableFlowCanvas from './EditableFlowCanvas';
import './flowCanvasStyles.css';
import FlowNodeComponent from './FlowNodeComponent';

export interface FlowCanvasProps {
  nodes: FlowNode[];
  edges: FlowEdge[];
  /**
   * Enable the editable builder (drag/connect/add/delete/save). Defaults to
   * `false` — the read-only viewer that ships everywhere else stays intact.
   */
  editable?: boolean;
  /** Graph-level metadata needed to re-serialize on save (editable only). */
  meta?: WorkflowGraphMeta;
  /** Save callback: receives the live canvas as a `WorkflowGraph` (editable only). */
  onSave?: (graph: WorkflowGraph) => void;
}

const NODE_TYPES = { [FLOW_NODE_TYPE]: FlowNodeComponent };

/**
 * Read-only render path — unchanged from B5b.1. Kept a separate component so
 * its `useMemo` hook stays unconditional and the editable path's controlled
 * state hooks never run for a plain viewer.
 */
function ReadonlyFlowCanvas({ nodes, edges }: { nodes: FlowNode[]; edges: FlowEdge[] }) {
  const interactionProps = useMemo(
    () => ({ nodesDraggable: false, nodesConnectable: false, elementsSelectable: false }),
    []
  );

  return (
    <div className="flow-canvas h-full w-full" data-testid="flow-canvas">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={NODE_TYPES}
        fitView
        panOnScroll
        zoomOnScroll
        {...interactionProps}>
        <Background variant={BackgroundVariant.Dots} gap={16} size={1} />
        <MiniMap pannable zoomable />
        <Controls showInteractive={false} />
      </ReactFlow>
    </div>
  );
}

/**
 * Fills its parent's box (`h-full w-full` — the page decides how tall/wide
 * that is; `FlowCanvasPage` gives it the full panel body).
 */
function FlowCanvas({ nodes, edges, editable = false, meta, onSave }: FlowCanvasProps) {
  if (editable) {
    return (
      <EditableFlowCanvas
        nodes={nodes}
        edges={edges}
        meta={meta ?? { schema_version: 1, name: '' }}
        onSave={onSave}
      />
    );
  }
  return <ReadonlyFlowCanvas nodes={nodes} edges={edges} />;
}

export default memo(FlowCanvas);
