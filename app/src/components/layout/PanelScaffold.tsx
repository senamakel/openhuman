import type { ReactNode } from 'react';

import PanelHeader, { DEFAULT_PANEL_HEADER_BG, DEFAULT_PANEL_HEADER_CLASS } from './PanelHeader';

export interface PanelScaffoldProps {
  /**
   * Fixed header title. Optional — omit (along with the other header slots) for
   * a header-less, body-only scaffold.
   */
  title?: ReactNode;
  /** Fixed sub-title rendered under the title in a muted tone. */
  description?: ReactNode;
  /** Leading node before the title (e.g. a back button); brings its own spacing. */
  leading?: ReactNode;
  /** Right-aligned header action(s) (e.g. a refresh or "add" button). */
  action?: ReactNode;
  /**
   * Extra content pinned inside the fixed header, below the description — e.g. a
   * {@link ChipTabs} row that should stay visible while the body scrolls.
   */
  headerExtra?: ReactNode;
  /** Scrollable body content. */
  children: ReactNode;
  /** Extra classes on the scaffold root. */
  className?: string;
  /**
   * Classes for the scrollable body wrapper. Defaults to the canonical settings
   * spacing (`p-4 pt-2 space-y-5`); pass `''` when the body already supplies its
   * own padding (e.g. an embedded sub-panel).
   */
  contentClassName?: string;
  /** Classes for the fixed header band. */
  headerClassName?: string;
  /** Background applied to the fixed header band. */
  headerBgClassName?: string;
  testId?: string;
}

const DEFAULT_CONTENT_CLASS = 'p-4 pt-2 space-y-5';

/**
 * Standard scaffold: a fixed header ({@link PanelHeader}) carrying an optional
 * title + description (plus leading/action/headerExtra slots) above a scrollable
 * body. The header never scrolls; only `children` do.
 *
 * The scaffold fills its parent's height and owns the *only* vertical scroll in
 * its subtree — relying on an unbroken height chain from a bounded ancestor (in
 * settings, the two-pane content pane). With no bounded height it degrades
 * gracefully: the body grows and the nearest ancestor scroller takes over.
 *
 * Presentational. For the full page pattern (page title/description + chips over
 * one or more scaffolds), use {@link PanelPage}, which composes this.
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
  headerClassName = DEFAULT_PANEL_HEADER_CLASS,
  headerBgClassName = DEFAULT_PANEL_HEADER_BG,
  testId,
}: PanelScaffoldProps) {
  const hasHeader =
    title != null ||
    description != null ||
    leading != null ||
    action != null ||
    headerExtra != null;

  return (
    <div className={`relative flex h-full min-h-0 flex-col ${className}`} data-testid={testId}>
      {hasHeader && (
        <PanelHeader
          title={title}
          description={description}
          leading={leading}
          action={action}
          className={`flex-shrink-0 ${headerClassName}`}
          bgClassName={headerBgClassName}>
          {headerExtra}
        </PanelHeader>
      )}

      <div className={`min-h-0 flex-1 overflow-y-auto ${contentClassName}`}>{children}</div>
    </div>
  );
}
