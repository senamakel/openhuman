/**
 * FlowCanvas — the read-only Workflow Canvas view (issue B5b.1): renders a
 * saved flow's `WorkflowGraph` (already converted to xyflow's shape by
 * `graphAdapter.ts`) with a minimap, zoom/pan controls, and a dotted
 * background. This is the first slice of the visual builder — editing
 * (dragging nodes, drawing new edges) lands in B5b.2+; here every
 * interaction that would mutate the graph is disabled.
 */
import { Background, BackgroundVariant, Controls, MiniMap, ReactFlow } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { memo, useMemo } from 'react';

import { FLOW_NODE_TYPE, type FlowEdge, type FlowNode } from '../../../lib/flows/graphAdapter';
import './flowCanvasStyles.css';
import FlowNodeComponent from './FlowNodeComponent';

export interface FlowCanvasProps {
  nodes: FlowNode[];
  edges: FlowEdge[];
  /**
   * Whether the canvas allows editing. Defaults to `true` (read-only) since
   * this slice ships no editing UI at all — B5b.2+ will pass `false` once an
   * editor exists.
   */
  readonly?: boolean;
}

const NODE_TYPES = { [FLOW_NODE_TYPE]: FlowNodeComponent };

/**
 * Fills its parent's box (`h-full w-full` — the page decides how tall/wide
 * that is; `FlowCanvasPage` gives it the full panel body).
 */
function FlowCanvas({ nodes, edges, readonly = true }: FlowCanvasProps) {
  const interactionProps = useMemo(
    () =>
      readonly ? { nodesDraggable: false, nodesConnectable: false, elementsSelectable: false } : {},
    [readonly]
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

export default memo(FlowCanvas);
