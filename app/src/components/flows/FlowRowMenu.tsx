/**
 * FlowRowMenu — the "⋯" overflow menu on a Workflows list row. Holds the
 * secondary/rare actions (Export, Duplicate, Delete) so the row's primary
 * actions (View runs, Run) stay uncluttered, and keeps the destructive Delete
 * out of the flat button row. Closes on Escape, outside click, or item select.
 *
 * Presentational + local open state only — each item calls back up to
 * `FlowListRow`, which routes to `FlowsPage`'s handlers.
 */
import { useCallback, useEffect, useRef, useState } from 'react';

import { useEscapeKey } from '../../hooks/useEscapeKey';
import { useT } from '../../lib/i18n/I18nContext';

export interface FlowRowMenuItem {
  key: string;
  label: string;
  onSelect: () => void;
  /** Renders the item in the destructive coral tone (e.g. Delete). */
  danger?: boolean;
  testId?: string;
}

export interface FlowRowMenuProps {
  items: FlowRowMenuItem[];
  /** Suffixed onto test ids so multiple rows stay addressable. */
  rowId: string;
}

function KebabIcon() {
  return (
    <svg className="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <circle cx="12" cy="5" r="1.6" />
      <circle cx="12" cy="12" r="1.6" />
      <circle cx="12" cy="19" r="1.6" />
    </svg>
  );
}

export default function FlowRowMenu({ items, rowId }: FlowRowMenuProps) {
  const { t } = useT();
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEscapeKey(() => setOpen(false), open);

  // Close on any click outside the menu container.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      if (!containerRef.current?.contains(event.target as Node | null)) setOpen(false);
    };
    document.addEventListener('mousedown', onPointerDown);
    return () => document.removeEventListener('mousedown', onPointerDown);
  }, [open]);

  const select = useCallback((onSelect: () => void) => {
    setOpen(false);
    onSelect();
  }, []);

  return (
    <div className="relative" ref={containerRef}>
      <button
        type="button"
        data-testid={`flow-menu-${rowId}`}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t('flows.list.moreActions')}
        title={t('flows.list.moreActions')}
        onClick={() => setOpen(o => !o)}
        className="flex h-8 w-8 items-center justify-center rounded-lg border border-line text-content-muted transition-colors hover:bg-surface-hover hover:text-content-secondary">
        <KebabIcon />
      </button>

      {open && (
        <div
          role="menu"
          data-testid={`flow-menu-list-${rowId}`}
          className="absolute right-0 z-20 mt-1 min-w-[10rem] overflow-hidden rounded-xl border border-line bg-surface py-1 shadow-lg">
          {items.map(item => (
            <button
              key={item.key}
              type="button"
              role="menuitem"
              data-testid={item.testId}
              onClick={() => select(item.onSelect)}
              className={`block w-full px-3 py-1.5 text-left text-xs transition-colors hover:bg-surface-hover ${
                item.danger ? 'text-coral-600 dark:text-coral-400' : 'text-content-secondary'
              }`}>
              {item.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
