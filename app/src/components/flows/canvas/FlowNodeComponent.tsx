/**
 * FlowNodeComponent — the custom xyflow node renderer for the read-only
 * Workflow Canvas (issue B5b.1). Renders one rounded card per `WorkflowNode`:
 * a per-kind emoji + colored accent, the node's name, and a `Handle` per
 * effective input port (left) / output port (right) — see
 * `graphAdapter.ts`'s `FlowNodeData` for why "effective" ports aren't simply
 * `data.ports`.
 *
 * Emoji (not an icon library) matches the repo's existing convention —
 * there is no `lucide-react` (or any icon-font) dependency in this app today
 * (icons are hand-rolled inline SVG, see `components/ui/icons.tsx`), and
 * adding one is out of scope for this slice's single approved dependency
 * (`@xyflow/react`).
 *
 * An unrecognized `kind` (not one of the 12 `NodeKind` values — e.g. a future
 * tinyflows addition, since `Flow.graph` is `unknown` on the wire) renders as
 * a plain neutral node rather than throwing, since a thrown render error here
 * has no error boundary around `<ReactFlow>` and would take down the whole
 * canvas.
 */
import { Handle, type NodeProps, Position } from '@xyflow/react';
import { memo } from 'react';

import type { FlowNode } from '../../../lib/flows/graphAdapter';
import type { NodeKind } from '../../../lib/flows/types';
import { useT } from '../../../lib/i18n/I18nContext';

type NodeColor = 'sage' | 'primary' | 'amber' | 'coral' | 'neutral';

/** Per-kind emoji + border/chip color. Colors cycle through the four
 * CSS-variable-backed semantic ramps (primary/sage/amber/coral) that support
 * Tailwind's `/opacity` modifiers in this codebase (see `tailwind.config.js`)
 * so light/dark theming comes for free; with 12 kinds and 4 ramps some kinds
 * share a color family; the emoji + name remain the primary distinguishers.
 *
 * `data.kind` is typed as the 12-entry `NodeKind` union, but a saved graph is
 * `unknown` on the wire (cast in `FlowCanvasPage.tsx`) — a future 13th
 * tinyflows kind, or any other value the backend ever emits, can reach this
 * map at runtime even though TypeScript can't see it. Index lookups below
 * fall back to {@link DEFAULT_NODE_META} rather than assuming a hit, so an
 * unrecognized kind renders as a plain neutral node instead of crashing the
 * whole canvas (there's no error boundary around `<ReactFlow>`).
 */
const NODE_KIND_META: Record<NodeKind, { emoji: string; color: NodeColor }> = {
  trigger: { emoji: '⚡', color: 'sage' },
  agent: { emoji: '🤖', color: 'primary' },
  tool_call: { emoji: '🔧', color: 'amber' },
  http_request: { emoji: '🌐', color: 'coral' },
  code: { emoji: '📝', color: 'sage' },
  condition: { emoji: '🔀', color: 'primary' },
  switch: { emoji: '🔁', color: 'amber' },
  merge: { emoji: '🔗', color: 'coral' },
  split_out: { emoji: '📤', color: 'sage' },
  transform: { emoji: '♻️', color: 'primary' },
  output_parser: { emoji: '📋', color: 'amber' },
  sub_workflow: { emoji: '🧩', color: 'coral' },
};

/** Fallback for any `kind` outside the map above — see the doc comment on `NODE_KIND_META`. */
const DEFAULT_NODE_META: { emoji: string; color: NodeColor } = { emoji: '❔', color: 'neutral' };

const COLOR_CLASSES: Record<NodeColor, { border: string; chip: string }> = {
  sage: {
    border: 'border-sage-400 dark:border-sage-500/60',
    chip: 'bg-sage-100 dark:bg-sage-500/20',
  },
  primary: {
    border: 'border-primary-400 dark:border-primary-500/60',
    chip: 'bg-primary-100 dark:bg-primary-500/20',
  },
  amber: {
    border: 'border-amber-400 dark:border-amber-500/60',
    chip: 'bg-amber-100 dark:bg-amber-500/20',
  },
  coral: {
    border: 'border-coral-400 dark:border-coral-500/60',
    chip: 'bg-coral-100 dark:bg-coral-500/20',
  },
  neutral: { border: 'border-line-strong', chip: 'bg-surface-subtle' },
};

/** Even vertical offsets (in %) for `count` handles along one side of the card. */
function handleOffsets(count: number): number[] {
  if (count <= 1) return [50];
  return Array.from({ length: count }, (_, i) => ((i + 1) / (count + 1)) * 100);
}

function FlowNodeComponent({ data, selected }: NodeProps<FlowNode>) {
  const { t } = useT();
  const meta = NODE_KIND_META[data.kind] ?? DEFAULT_NODE_META;
  const colors = COLOR_CLASSES[meta.color];
  const inputOffsets = handleOffsets(data.inputPorts.length);
  const outputOffsets = handleOffsets(data.outputPorts.length);
  const kindLabel = t(`flows.nodeKind.${data.kind}`, data.kind);

  return (
    <div
      data-testid="flow-node"
      data-node-kind={data.kind}
      className={`relative min-w-[180px] max-w-[240px] rounded-xl border-2 bg-surface shadow-sm ${colors.border} ${
        selected ? 'ring-2 ring-primary-500/40' : ''
      }`}>
      {data.inputPorts.map((port, i) => (
        <Handle
          key={`in-${port}`}
          id={port}
          type="target"
          position={Position.Left}
          style={{ top: `${inputOffsets[i]}%` }}
          title={port}
        />
      ))}

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

      {data.outputPorts.length > 1 && (
        <div className="space-y-0.5 px-3 py-2 text-[10px] text-content-faint">
          {data.outputPorts.map(port => (
            <div key={port} className="truncate">
              {port}
            </div>
          ))}
        </div>
      )}

      {data.outputPorts.map((port, i) => (
        <Handle
          key={`out-${port}`}
          id={port}
          type="source"
          position={Position.Right}
          style={{ top: `${outputOffsets[i]}%` }}
          title={port}
        />
      ))}
    </div>
  );
}

export default memo(FlowNodeComponent);
