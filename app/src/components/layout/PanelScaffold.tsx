import type { ReactNode } from 'react';

export interface PanelScaffoldProps {
  /**
   * Fixed header title. Optional — omit (along with the other header slots) for
   * a header-less, body-only panel.
   */
  title?: ReactNode;
  /** Fixed sub-title rendered under the title in a muted tone. */
  description?: ReactNode;
  /**
   * Leading node before the title (e.g. a back button). Bring your own spacing
   * — the title sits immediately after it.
   */
  leading?: ReactNode;
  /** Right-aligned header action(s) (e.g. a refresh or "add" button). */
  action?: ReactNode;
  /**
   * Extra content pinned inside the fixed header, below the title row — e.g. a
   * {@link ChipTabs} row that should stay visible while the body scrolls.
   */
  headerExtra?: ReactNode;
  /** Scrollable body content. */
  children: ReactNode;
  /** Extra classes on the panel root. */
  className?: string;
  /**
   * Classes for the scrollable body wrapper. Defaults to the canonical settings
   * spacing (`p-4 pt-2 space-y-5`); pass `''` when the body already supplies its
   * own padding (e.g. an embedded sub-panel).
   */
  contentClassName?: string;
  /** Classes for the fixed (sticky) header wrapper. */
  headerClassName?: string;
  /**
   * Background applied to the sticky header so scrolling content is hidden
   * behind it. Must match the surrounding pane. Defaults to the standard pane
   * surface; pass `''` to opt out (e.g. a transparent host).
   */
  headerBgClassName?: string;
  testId?: string;
}

const DEFAULT_HEADER_CLASS = 'px-5 pt-5 pb-3';
const DEFAULT_HEADER_BG = 'bg-white dark:bg-neutral-900';
const DEFAULT_CONTENT_CLASS = 'p-4 pt-2 space-y-5';

/**
 * Standard right-pane panel scaffold: a fixed (sticky) header carrying an
 * optional title + description (and optional leading/action/tab slots) above a
 * scrollable body. Built so every settings/detail panel shares the same header
 * spacing and scroll behavior instead of hand-rolling a header + content `div`.
 *
 * The header sticks to the nearest scrolling ancestor (the two-pane content
 * pane in settings), so it stays put while `children` scroll beneath it. The
 * component is presentational — hosts own the data and the back/action nodes.
 */
export default function PanelScaffold({
  title,
  description,
  leading,
  action,
  headerExtra,
  children,
  className = '',
  contentClassName = DEFAULT_CONTENT_CLASS,
  headerClassName = DEFAULT_HEADER_CLASS,
  headerBgClassName = DEFAULT_HEADER_BG,
  testId,
}: PanelScaffoldProps) {
  const hasTitleRow = title != null || leading != null || action != null;
  const hasHeader = hasTitleRow || description != null || headerExtra != null;

  return (
    <div className={`relative ${className}`} data-testid={testId}>
      {hasHeader && (
        <div className={`sticky top-0 z-20 ${headerBgClassName} ${headerClassName}`}>
          {hasTitleRow && (
            <div className="flex items-center justify-between gap-2">
              <div className="flex min-w-0 items-center">
                {leading}
                {title != null && (
                  <h2 className="text-base font-semibold text-stone-900 dark:text-neutral-100">
                    {title}
                  </h2>
                )}
              </div>
              {action != null && <div className="flex-shrink-0">{action}</div>}
            </div>
          )}

          {description != null && (
            <p className="mt-1 text-sm text-stone-500 dark:text-neutral-400">{description}</p>
          )}

          {headerExtra}
        </div>
      )}

      <div className={contentClassName}>{children}</div>
    </div>
  );
}
