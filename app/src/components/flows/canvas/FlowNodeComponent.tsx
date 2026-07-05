/**
 * FlowNodeComponent — the custom xyflow node renderer for the Workflow Canvas
 * (issue B5b.1). Renders one rounded card per `WorkflowNode`: a per-kind emoji +
 * colored accent header, the node's name, and a labelled row per input port
 * (left) / output port (right) — see `graphAdapter.ts`'s `FlowNodeData` for why
 * "effective" ports aren't simply `data.ports`.
 *
 * Ports read as labelled handle rows rather than a plaintext list: each port's
 * `Handle` sits inline next to its name so it's unambiguous which dot carries
 * which input/output (e.g. a `condition`'s `true`/`false` outputs, a `switch`'s
 * case ports). Branch ports are colour-coded (true → sage, false/error → coral)
 * so the routing reads at a glance. A lone implicit `main` port shows just its
 * handle dot — left = input, right = output, the flow-editor convention — with
 * no redundant "main" label.
 *
 * Emoji (not an icon library) matches the repo's existing convention. An
 * unrecognized `kind` (not one of the 12 `NodeKind` values) renders as a plain
 * neutral node rather than throwing, since a thrown render error here has no
 * error boundary around `<ReactFlow>` and would take down the whole canvas.
 */
import { Handle, type NodeProps, Position } from '@xyflow/react';
import { type CSSProperties, memo } from 'react';

import type { FlowNode } from '../../../lib/flows/graphAdapter';
import { COLOR_CLASSES, nodeKindMeta } from '../../../lib/flows/nodeKindMeta';
import { useT } from '../../../lib/i18n/I18nContext';

/**
 * Inline the handle into the port row instead of React Flow's default absolute
 * edge placement, so each dot flows next to its label. React Flow still derives
 * the connection point from the handle's measured position, so edges attach
 * correctly.
 */
const INLINE_HANDLE_STYLE: CSSProperties = {
  position: 'relative',
  top: 'auto',
  left: 'auto',
  right: 'auto',
  transform: 'none',
};

/** The implicit single port; shown as a bare dot with no redundant label. */
const IMPLICIT_PORT = 'main';

/** Semantic colours for the well-known branch ports so routing reads at a glance. */
function portPillClass(port: string): string {
  const base = 'rounded px-1.5 py-0.5 text-[10px] font-medium leading-none';
  const key = port.toLowerCase();
  if (key === 'true') {
    return `${base} bg-sage-100 text-sage-700 dark:bg-sage-500/20 dark:text-sage-300`;
  }
  if (key === 'false' || key === 'error') {
    return `${base} bg-coral-100 text-coral-700 dark:bg-coral-500/20 dark:text-coral-300`;
  }
  return `${base} bg-surface-subtle text-content-secondary`;
}

function FlowNodeComponent({ data, selected }: NodeProps<FlowNode>) {
  const { t } = useT();
  const meta = nodeKindMeta(data.kind);
  const colors = COLOR_CLASSES[meta.color];
  const kindLabel = t(`flows.nodeKind.${data.kind}`, data.kind);

  // Only label ports when there's something to disambiguate: more than one port,
  // or a single explicitly-named (non-`main`) port. A lone implicit `main` shows
  // just its dot.
  const labelInputs = data.inputPorts.length > 1 || data.inputPorts.some(p => p !== IMPLICIT_PORT);
  const labelOutputs =
    data.outputPorts.length > 1 || data.outputPorts.some(p => p !== IMPLICIT_PORT);
  const hasPorts = data.inputPorts.length > 0 || data.outputPorts.length > 0;

  return (
    <div
      data-testid="flow-node"
      data-node-kind={data.kind}
      className={`relative min-w-[180px] max-w-[240px] rounded-xl border-2 bg-surface shadow-sm ${colors.border} ${
        selected ? 'ring-2 ring-primary-500/40' : ''
      }`}>
      <div className={`flex items-center gap-2 rounded-t-[10px] px-3 py-2 ${colors.chip}`}>
        <span className="text-base leading-none" aria-hidden="true">
          {meta.emoji}
        </span>
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-content">{data.name}</div>
          <div className="truncate text-[10px] uppercase tracking-wide text-content-faint">
            {kindLabel}
          </div>
        </div>
      </div>

      {hasPorts && (
        <div className="flex items-start justify-between gap-4 px-2 py-2">
          {/* Inputs — handle on the left edge, label to its right. */}
          <div className="flex min-w-0 flex-col gap-1.5">
            {data.inputPorts.map(port => (
              <div key={`in-${port}`} className="flex items-center gap-1.5">
                <Handle
                  id={port}
                  type="target"
                  position={Position.Left}
                  style={INLINE_HANDLE_STYLE}
                  title={port}
                />
                {labelInputs && <span className={`truncate ${portPillClass(port)}`}>{port}</span>}
              </div>
            ))}
          </div>

          {/* Outputs — label first, handle on the right edge. */}
          <div className="flex min-w-0 flex-col items-end gap-1.5">
            {data.outputPorts.map(port => (
              <div key={`out-${port}`} className="flex items-center gap-1.5">
                {labelOutputs && <span className={`truncate ${portPillClass(port)}`}>{port}</span>}
                <Handle
                  id={port}
                  type="source"
                  position={Position.Right}
                  style={INLINE_HANDLE_STYLE}
                  title={port}
                />
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export default memo(FlowNodeComponent);
