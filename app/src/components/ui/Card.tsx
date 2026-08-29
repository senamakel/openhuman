import { type ReactNode } from 'react';

import { cn } from '../../lib/cn';

export interface CardProps {
  title?: string;
  description?: string;
  children: ReactNode;
  className?: string;
  'data-testid'?: string;
}

/**
 * A bordered surface with an optional heading and divided body — the shape
 * ~470 hand-rolled `rounded-* border bg-*` wrappers across the app are
 * reproducing. Generalized out of `settings/controls/SettingsSection`, which
 * now re-exports this.
 */
const Card = ({ title, description, children, className, 'data-testid': testId }: CardProps) => (
  <div
    data-slot="card"
    data-testid={testId}
    className={cn('overflow-hidden rounded-xl border border-line bg-surface', className)}>
    {title && (
      <div className="px-4 pb-0 pt-4">
        {/* Real heading (h3, one level below SettingsHeader's h2) for a11y and
            so getByRole('heading') keeps resolving section titles. */}
        <h3 className="text-xs font-semibold tracking-wide text-content-muted">{title}</h3>
        {description && (
          <p className="mt-1 text-xs leading-relaxed text-content-muted">{description}</p>
        )}
      </div>
    )}
    {/* `divide-line-subtle` flips with the theme on its own, so the historical
        hardcoded dark-mode companion is gone: a raw palette scale would not
        follow a user's custom theme. */}
    <div className="divide-y divide-line-subtle">{children}</div>
  </div>
);

export default Card;
