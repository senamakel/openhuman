import { type ReactNode, useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import { useAppDispatch, useAppSelector } from '../../../store/hooks';
import { ensurePanelLayout, selectPanelLayout, setSidebarWidth } from '../../../store/layoutSlice';

const LAYOUT_ID = 'root-shell';
const DEFAULT_WIDTH = 224;
const MIN_WIDTH = 188;
const MAX_WIDTH = 420;
const KEYBOARD_STEP = 16;

function clamp(width: number): number {
  return Math.min(Math.max(width, MIN_WIDTH), MAX_WIDTH);
}

export interface RootShellLayoutProps {
  /** Always-visible left pane (the app sidebar). */
  sidebar: ReactNode;
  /** Dynamic main content (the routed page area). */
  children: ReactNode;
}

/**
 * Full-bleed, viewport-filling two-pane shell for the app root: a resizable
 * sidebar on the left and the main content on the right, separated by a flush
 * hairline seam. Unlike the in-page {@link TwoPanelLayout}, this fills its
 * container edge-to-edge (no card, no rounded corners) because it *is* the
 * window chrome. The dragged width persists per user via the `layout` slice
 * (id `root-shell`); the sidebar is always shown.
 */
export default function RootShellLayout({ sidebar, children }: RootShellLayoutProps) {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const layout = useAppSelector(selectPanelLayout(LAYOUT_ID, { sidebarWidth: DEFAULT_WIDTH }));
  const persistedWidth = clamp(layout.sidebarWidth);

  // Seed persisted geometry once so the selector returns a stable stored
  // reference on subsequent renders (avoids the new-object memoization warning).
  useEffect(() => {
    dispatch(ensurePanelLayout({ id: LAYOUT_ID, defaults: { sidebarWidth: DEFAULT_WIDTH } }));
  }, [dispatch]);

  const [dragWidth, setDragWidth] = useState<number | null>(null);
  const dragWidthRef = useRef<number | null>(null);
  const dragCleanupRef = useRef<(() => void) | null>(null);
  const width = dragWidth ?? persistedWidth;

  const commitWidth = useCallback(
    (next: number) => dispatch(setSidebarWidth({ id: LAYOUT_ID, width: clamp(Math.round(next)) })),
    [dispatch]
  );

  const onPointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      const startX = e.clientX;
      const startWidth = width;
      dragWidthRef.current = startWidth;
      setDragWidth(startWidth);
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';

      function handleMove(ev: PointerEvent) {
        const next = clamp(startWidth + (ev.clientX - startX));
        dragWidthRef.current = next;
        setDragWidth(next);
      }
      function detach() {
        window.removeEventListener('pointermove', handleMove);
        window.removeEventListener('pointerup', stop);
        document.body.style.removeProperty('cursor');
        document.body.style.removeProperty('user-select');
        dragCleanupRef.current = null;
      }
      function stop() {
        detach();
        const finalWidth = dragWidthRef.current;
        dragWidthRef.current = null;
        setDragWidth(null);
        if (finalWidth != null) commitWidth(finalWidth);
      }

      dragCleanupRef.current = detach;
      window.addEventListener('pointermove', handleMove);
      window.addEventListener('pointerup', stop);
    },
    [width, commitWidth]
  );

  // Detach global listeners if we unmount mid-drag.
  useLayoutEffect(() => () => dragCleanupRef.current?.(), []);

  const onDividerKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'ArrowLeft') {
        e.preventDefault();
        commitWidth(persistedWidth - KEYBOARD_STEP);
      } else if (e.key === 'ArrowRight') {
        e.preventDefault();
        commitWidth(persistedWidth + KEYBOARD_STEP);
      }
    },
    [commitWidth, persistedWidth]
  );

  return (
    <div className="flex h-full w-full min-h-0 overflow-hidden">
      <div
        className="flex-shrink-0 min-w-0 overflow-hidden"
        style={{ width }}
        data-testid="root-shell-sidebar">
        {sidebar}
      </div>

      <div
        role="separator"
        aria-orientation="vertical"
        aria-label={t('layout.resizeSidebar')}
        aria-valuenow={Math.round(width)}
        aria-valuemin={MIN_WIDTH}
        aria-valuemax={MAX_WIDTH}
        tabIndex={0}
        data-testid="root-shell-divider"
        data-analytics-id="root-shell-resize-divider"
        onPointerDown={onPointerDown}
        onKeyDown={onDividerKeyDown}
        title={t('layout.resizeSidebar')}
        className="group relative w-px flex-shrink-0 cursor-col-resize select-none self-stretch bg-stone-200 dark:bg-neutral-800 focus:outline-none">
        <span className="absolute inset-y-0 -left-1 -right-1 z-10" />
        <span className="absolute inset-0 transition-colors group-hover:bg-primary-400 group-focus:bg-primary-500" />
      </div>

      <div className="flex-1 min-w-0 overflow-hidden" data-testid="root-shell-content">
        {children}
      </div>
    </div>
  );
}
