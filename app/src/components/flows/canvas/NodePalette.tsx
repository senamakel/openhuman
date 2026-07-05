/**
 * NodePalette — the editable Workflow Canvas's insert palette (issue B5b.2 /
 * Phase 3a). Lists the tinyflows `NodeKind`s with the same emoji + accent
 * `FlowNodeComponent` renders (both pull from `lib/flows/nodeKindMeta.ts`),
 * grouped into labelled sections (Triggers / Actions / Logic) with a search
 * filter so the 12 kinds stay scannable. Two ways to add a node:
 *
 *  - **click** an entry → `onAdd(kind)` (the canvas drops it at a default
 *    position). Keyboard-accessible and the path the unit tests drive, since
 *    jsdom can't produce real drag geometry.
 *  - **drag** an entry onto the canvas → sets a `application/tinyflows-node`
 *    dataTransfer payload the canvas's `onDrop` reads to place the node under
 *    the cursor. Pointer-only affordance, layered on top of click.
 */
import { memo, useMemo, useState } from 'react';

import {
  COLOR_CLASSES,
  NODE_GROUP_ORDER,
  NODE_KINDS_BY_GROUP,
  nodeKindMeta,
} from '../../../lib/flows/nodeKindMeta';
import type { NodeKind } from '../../../lib/flows/types';
import { useT } from '../../../lib/i18n/I18nContext';

/** dataTransfer MIME key for a palette drag — read by the canvas `onDrop`. */
export const PALETTE_DND_MIME = 'application/tinyflows-node';

export interface NodePaletteProps {
  /** Add a node of `kind` at the canvas's default insert position (click path). */
  onAdd: (kind: NodeKind) => void;
}

function NodePalette({ onAdd }: NodePaletteProps) {
  const { t } = useT();
  const [query, setQuery] = useState('');

  // Label each kind once (memoised) so filtering matches on the localized name.
  const labelFor = useMemo(
    () => (kind: NodeKind) => t(`flows.nodeKind.${kind}`, kind),
    [t]
  );

  const needle = query.trim().toLowerCase();
  // Groups with their matching kinds; groups that filter down to nothing are
  // dropped so empty section headers don't linger while searching.
  const groups = NODE_GROUP_ORDER.map(group => ({
    group,
    kinds: NODE_KINDS_BY_GROUP[group].filter(
      kind => needle === '' || labelFor(kind).toLowerCase().includes(needle)
    ),
  })).filter(section => section.kinds.length > 0);

  return (
    <aside
      className="pointer-events-auto absolute left-3 top-3 z-10 flex max-h-[calc(100%-1.5rem)] w-48 flex-col overflow-hidden rounded-xl border border-line bg-surface/95 shadow-sm backdrop-blur"
      data-testid="flow-node-palette"
      aria-label={t('flows.palette.title')}>
      <div className="border-b border-line px-3 py-2 text-[11px] font-semibold uppercase tracking-wide text-content-faint">
        {t('flows.palette.title')}
      </div>
      <div className="border-b border-line p-2">
        <input
          type="search"
          value={query}
          onChange={e => setQuery(e.target.value)}
          data-testid="flow-palette-search"
          placeholder={t('flows.palette.search')}
          aria-label={t('flows.palette.search')}
          className="w-full rounded-lg border border-line bg-surface px-2 py-1 text-xs text-content placeholder:text-content-faint focus:border-primary-400 focus:outline-none"
        />
      </div>
      <div className="flex flex-col gap-2 overflow-y-auto p-2">
        {groups.length === 0 && (
          <p
            className="px-1 py-2 text-center text-[11px] text-content-faint"
            data-testid="flow-palette-no-results">
            {t('flows.palette.noResults')}
          </p>
        )}
        {groups.map(({ group, kinds }) => (
          <div key={group} className="flex flex-col gap-1">
            <div className="px-1 text-[10px] font-semibold uppercase tracking-wide text-content-faint">
              {t(`flows.palette.group.${group}`)}
            </div>
            {kinds.map(kind => {
              const meta = nodeKindMeta(kind);
              const colors = COLOR_CLASSES[meta.color];
              const label = labelFor(kind);
              return (
                <button
                  key={kind}
                  type="button"
                  draggable
                  data-testid={`flow-palette-item-${kind}`}
                  data-node-kind={kind}
                  onClick={() => onAdd(kind)}
                  onDragStart={event => {
                    event.dataTransfer.setData(PALETTE_DND_MIME, kind);
                    event.dataTransfer.effectAllowed = 'copy';
                  }}
                  title={t('flows.palette.addNode').replace('{kind}', label)}
                  className={`flex items-center gap-2 rounded-lg border px-2 py-1.5 text-left text-xs text-content transition-colors hover:bg-surface-hover ${colors.border}`}>
                  <span className="text-base leading-none" aria-hidden="true">
                    {meta.emoji}
                  </span>
                  <span className="truncate">{label}</span>
                </button>
              );
            })}
          </div>
        ))}
      </div>
    </aside>
  );
}

export default memo(NodePalette);
