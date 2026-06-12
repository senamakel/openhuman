import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import {
  ensurePanelLayout,
  selectPanelLayout,
  setSidebarVisible,
  setSidebarWidth,
  toggleSidebar,
  type PanelLayout,
} from '../../store/layoutSlice';
import { useAppDispatch, useAppSelector } from '../../store/hooks';

const namespace = 'two-panel-layout';

function debug(message: string, payload?: Record<string, unknown>) {
  if (import.meta.env.DEV) {
    // eslint-disable-next-line no-console
    console.debug(`[${namespace}] ${message}`, payload ?? {});
  }
}

function clampWidth(width: number, min: number, max: number): number {
  return Math.min(Math.max(width, min), max);
}

/**
 * Subscribe to a two-pane layout's persisted geometry and get back the
 * helpers external chrome needs to drive it (e.g. a hamburger button living
 * in some other header). Reads the SAME slice state `TwoPanelLayout` renders
 * from, so toggles stay in sync.
 */
export function useTwoPanelLayout(id: string, defaults?: Partial<PanelLayout>) {
  const dispatch = useAppDispatch();
  const layout = useAppSelector(selectPanelLayout(id, defaults));

  const show = useCallback(
    (visible: boolean) => dispatch(setSidebarVisible({ id, visible })),
    [dispatch, id]
  );
  const toggle = useCallback(() => dispatch(toggleSidebar({ id })), [dispatch, id]);

  return {
    sidebarVisible: layout.sidebarVisible,
    sidebarWidth: layout.sidebarWidth,
    showSidebar: show,
    toggleSidebar: toggle,
  };
}

export interface TwoPanelLayoutProps {
  /** Stable id used as the persistence key for this layout's geometry. */
  id: string;
  /** Content of the mini sidebar (left pane). */
  sidebar: ReactNode;
  /** Main content (right pane). */
  children: ReactNode;
  /** Sidebar visibility on first ever mount (before any persisted state). */
  defaultSidebarVisible?: boolean;
  /** Sidebar width in px on first ever mount. */
  defaultSidebarWidth?: number;
  /** Minimum sidebar width while dragging. */
  minSidebarWidth?: number;
  /** Maximum sidebar width while dragging. */
  maxSidebarWidth?: number;
  /**
   * Force the sidebar open regardless of persisted state (e.g. an onboarding
   * lockdown where the sidebar must always show). The persisted preference is
   * untouched, so it restores once the force is lifted.
   */
  forceSidebarVisible?: boolean;
  /** Step (px) the keyboard divider moves per arrow press. */
  keyboardStep?: number;
  className?: string;
  sidebarClassName?: string;
  contentClassName?: string;
  /**
   * Show a thin rail with a reopen button when the sidebar is hidden. Defaults
   * to false because chat surfaces its own toggle in the header; standalone
   * uses can opt in.
   */
  showCollapsedRail?: boolean;
}

const DEFAULT_MIN_WIDTH = 180;
const DEFAULT_MAX_WIDTH = 480;
const DEFAULT_KEYBOARD_STEP = 16;

/**
 * A reusable two-pane shell: a resizable mini sidebar on the left and main
 * content on the right. Visibility and the dragged width persist per `id` via
 * the Redux `layout` slice, so the layout is remembered across reloads.
 *
 * Resize: drag the divider between the panes (pointer) or focus it and use the
 * arrow keys. Width is clamped to [minSidebarWidth, maxSidebarWidth] and only
 * committed to the store on drag end to avoid thrashing redux-persist.
 */
export default function TwoPanelLayout({
  id,
  sidebar,
  children,
  defaultSidebarVisible = false,
  defaultSidebarWidth,
  minSidebarWidth = DEFAULT_MIN_WIDTH,
  maxSidebarWidth = DEFAULT_MAX_WIDTH,
  forceSidebarVisible = false,
  keyboardStep = DEFAULT_KEYBOARD_STEP,
  className = '',
  sidebarClassName = '',
  contentClassName = '',
  showCollapsedRail = false,
}: TwoPanelLayoutProps) {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const layout = useAppSelector(
    selectPanelLayout(id, {
      sidebarVisible: defaultSidebarVisible,
      ...(defaultSidebarWidth != null ? { sidebarWidth: defaultSidebarWidth } : {}),
    })
  );

  // Seed persisted geometry from this component's defaults exactly once per id.
  useEffect(() => {
    dispatch(
      ensurePanelLayout({
        id,
        defaults: {
          sidebarVisible: defaultSidebarVisible,
          ...(defaultSidebarWidth != null ? { sidebarWidth: defaultSidebarWidth } : {}),
        },
      })
    );
    // Intentionally only on id change — defaults are a first-mount seed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  const isOpen = forceSidebarVisible || layout.sidebarVisible;

  // Live width while dragging is kept local (and applied via inline style) so
  // we don't dispatch — and re-persist — on every pointer move.
  const [dragWidth, setDragWidth] = useState<number | null>(null);
  const dragWidthRef = useRef<number | null>(null);
  const persistedWidth = clampWidth(layout.sidebarWidth, minSidebarWidth, maxSidebarWidth);
  const width = dragWidth ?? persistedWidth;

  const dragStartRef = useRef<{ startX: number; startWidth: number } | null>(null);

  const commitWidth = useCallback(
    (next: number) => {
      const clamped = clampWidth(Math.round(next), minSidebarWidth, maxSidebarWidth);
      dispatch(setSidebarWidth({ id, width: clamped }));
      debug('commit width', { id, width: clamped });
    },
    [dispatch, id, minSidebarWidth, maxSidebarWidth]
  );

  const onPointerMove = useCallback(
    (e: PointerEvent) => {
      const start = dragStartRef.current;
      if (!start) return;
      const next = clampWidth(
        start.startWidth + (e.clientX - start.startX),
        minSidebarWidth,
        maxSidebarWidth
      );
      dragWidthRef.current = next;
      setDragWidth(next);
    },
    [minSidebarWidth, maxSidebarWidth]
  );

  const onPointerUp = useCallback(() => {
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', onPointerUp);
    document.body.style.removeProperty('cursor');
    document.body.style.removeProperty('user-select');
    dragStartRef.current = null;
    const finalWidth = dragWidthRef.current;
    dragWidthRef.current = null;
    setDragWidth(null);
    if (finalWidth != null) commitWidth(finalWidth);
  }, [onPointerMove, commitWidth]);

  const onPointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      dragStartRef.current = { startX: e.clientX, startWidth: width };
      dragWidthRef.current = width;
      setDragWidth(width);
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      window.addEventListener('pointermove', onPointerMove);
      window.addEventListener('pointerup', onPointerUp);
      debug('drag start', { id, startWidth: width });
    },
    [width, onPointerMove, onPointerUp, id]
  );

  // Detach global listeners if we unmount mid-drag.
  useLayoutEffect(() => {
    return () => {
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
      document.body.style.removeProperty('cursor');
      document.body.style.removeProperty('user-select');
    };
  }, [onPointerMove, onPointerUp]);

  const onDividerKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'ArrowLeft') {
        e.preventDefault();
        commitWidth(persistedWidth - keyboardStep);
      } else if (e.key === 'ArrowRight') {
        e.preventDefault();
        commitWidth(persistedWidth + keyboardStep);
      }
    },
    [commitWidth, persistedWidth, keyboardStep]
  );

  return (
    <div className={`flex min-h-0 ${className}`}>
      {isOpen && (
        <>
          <div
            className={`flex-shrink-0 min-w-0 overflow-hidden ${sidebarClassName}`}
            style={{ width }}
            data-testid={`two-panel-sidebar-${id}`}>
            {sidebar}
          </div>

          {/* Drag handle / divider */}
          <div
            role="separator"
            aria-orientation="vertical"
            aria-label={t('layout.resizeSidebar')}
            aria-valuenow={Math.round(width)}
            aria-valuemin={minSidebarWidth}
            aria-valuemax={maxSidebarWidth}
            tabIndex={0}
            data-testid={`two-panel-divider-${id}`}
            data-analytics-id="two-panel-resize-divider"
            onPointerDown={onPointerDown}
            onKeyDown={onDividerKeyDown}
            className="group relative flex-shrink-0 w-2 mx-0.5 cursor-col-resize self-stretch focus:outline-none"
            title={t('layout.resizeSidebar')}>
            <span className="absolute inset-y-0 left-1/2 -translate-x-1/2 w-px bg-stone-200 dark:bg-neutral-800 transition-colors group-hover:bg-primary-400 group-focus:bg-primary-500" />
          </div>
        </>
      )}

      {!isOpen && showCollapsedRail && (
        <button
          type="button"
          data-testid={`two-panel-reopen-${id}`}
          data-analytics-id="two-panel-reopen-sidebar"
          onClick={() => dispatch(setSidebarVisible({ id, visible: true }))}
          title={t('layout.showSidebar')}
          aria-label={t('layout.showSidebar')}
          className="flex-shrink-0 w-6 self-stretch flex items-center justify-center text-stone-400 dark:text-neutral-500 hover:text-primary-500 hover:bg-stone-50 dark:hover:bg-neutral-800/60 transition-colors">
          <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
          </svg>
        </button>
      )}

      <div className={`flex-1 min-w-0 ${contentClassName}`}>{children}</div>
    </div>
  );
}
