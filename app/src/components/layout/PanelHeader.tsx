import type { ReactNode } from 'react';

export interface PanelHeaderProps {
  /** Header title. Optional — surrounding chrome (sidebar/chips) often names the view. */
  title?: ReactNode;
  /** Sub-title under the title, muted. */
  description?: ReactNode;
  /** Leading node before the title (e.g. a back button); brings its own spacing. */
  leading?: ReactNode;
  /** Right-aligned action(s) (e.g. refresh / add). */
  action?: ReactNode;
  /** Extra content rendered below the description (e.g. a chip row). */
  children?: ReactNode;
  /** Padding/layout classes for the band. */
  className?: string;
  /** Surface background for the band. */
  bgClassName?: string;
}

export const DEFAULT_PANEL_HEADER_CLASS = 'px-5 pt-5 pb-3';
export const DEFAULT_PANEL_HEADER_BG = 'bg-white dark:bg-neutral-900';

/**
 * The fixed header band shared by {@link PanelScaffold} (panel header) and
 * {@link PanelPage} (page chrome above the chips). Renders an optional title row
 * (leading + title + action), an optional description, and arbitrary extra
 * content below — all presentational, no scroll of its own.
 */
export default function PanelHeader({
  title,
  description,
  leading,
  action,
  children,
  className = DEFAULT_PANEL_HEADER_CLASS,
  bgClassName = DEFAULT_PANEL_HEADER_BG,
}: PanelHeaderProps) {
  const hasTitleRow = title != null || leading != null || action != null;

  return (
    <div className={`${bgClassName} ${className}`}>
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

      {children}
    </div>
  );
}
