import type { ReactNode } from 'react';

/**
 * PageSectionHeader — the canonical text-only header for a functional page view:
 * a title (16px semibold) over an optional one-line description (14px muted),
 * with an optional right-aligned action. Matches {@link PanelHeader}'s typography
 * so every screen — PanelPage-based or hand-rolled — reads the same.
 *
 * Use at the top of a content column on pages that don't route through
 * {@link PanelPage} (which renders the same title/description itself).
 */
export interface PageSectionHeaderProps {
  title: ReactNode;
  /** One-line description of what the view does. */
  description?: ReactNode;
  /** Right-aligned action(s) (e.g. buttons). */
  action?: ReactNode;
  className?: string;
  testId?: string;
}

export default function PageSectionHeader({
  title,
  description,
  action,
  className = '',
  testId,
}: PageSectionHeaderProps) {
  return (
    <header className={className} data-testid={testId}>
      <div className="flex items-start justify-between gap-3">
        <h1 className="text-base font-semibold text-content">{title}</h1>
        {action != null && <div className="flex-shrink-0">{action}</div>}
      </div>
      {description != null && <p className="mt-0.5 text-sm text-content-muted">{description}</p>}
    </header>
  );
}
