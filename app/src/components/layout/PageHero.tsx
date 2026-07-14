import type { ReactNode } from 'react';

/**
 * PageHero — the friendly "what is this screen" band at the top of a first-class
 * inner page. A soft gradient card with an accent illustration blob, a leading
 * icon/emoji tile, a title, a one-line benefit-led blurb, and an optional row of
 * benefit chips. Purely presentational.
 *
 * Standardises the intro across pages (Orchestration, Connections, Brain, …) so
 * every screen opens by telling the user what it does and why it helps, instead
 * of dropping them straight into controls. Feed it through {@link PanelPage}'s
 * `hero` slot so it sits in the fixed chrome above the tabs/body.
 */

/** Accent families available for the gradient + tile. Keys map to design tokens. */
export type PageHeroAccent = 'ocean' | 'sage' | 'amber' | 'coral';

/**
 * Static class strings per accent — Tailwind only scans literal class names, so
 * these must stay whole (no `from-${accent}` interpolation).
 */
const ACCENTS: Record<PageHeroAccent, { gradient: string; blob: string; tile: string }> = {
  ocean: {
    gradient: 'from-primary-500/15 via-primary-400/5',
    blob: 'bg-primary-400/25',
    tile: 'bg-primary-500/15 text-primary-600',
  },
  sage: {
    gradient: 'from-sage-500/15 via-sage-400/5',
    blob: 'bg-sage-400/25',
    tile: 'bg-sage-500/15 text-sage-600',
  },
  amber: {
    gradient: 'from-amber-500/15 via-amber-400/5',
    blob: 'bg-amber-400/25',
    tile: 'bg-amber-500/15 text-amber-600',
  },
  coral: {
    gradient: 'from-coral-500/15 via-coral-400/5',
    blob: 'bg-coral-400/25',
    tile: 'bg-coral-500/15 text-coral-600',
  },
};

export interface PageHeroBenefit {
  /** Leading glyph — an emoji string or a small icon node. */
  icon: ReactNode;
  /** Short benefit label (2–4 words). */
  label: ReactNode;
}

export interface PageHeroProps {
  /** Leading glyph shown in the accent tile — emoji string or icon node. */
  icon?: ReactNode;
  /** Page title. */
  title: ReactNode;
  /** One-line, benefit-led description of what the screen does. */
  description?: ReactNode;
  /** Optional benefit chips ("what you get"), rendered as pills below the blurb. */
  benefits?: PageHeroBenefit[];
  /** Optional right-aligned action (e.g. a primary CTA). */
  action?: ReactNode;
  /** Gradient/tile accent family. Defaults to `ocean` (the primary brand blue). */
  accent?: PageHeroAccent;
  className?: string;
  testId?: string;
}

export default function PageHero({
  icon,
  title,
  description,
  benefits,
  action,
  accent = 'ocean',
  className = '',
  testId,
}: PageHeroProps) {
  const a = ACCENTS[accent];

  return (
    <section
      data-testid={testId}
      className={`relative overflow-hidden rounded-2xl border border-line bg-gradient-to-br ${a.gradient} to-transparent px-5 py-4 shadow-soft ${className}`}>
      {/* Decorative illustration blob — soft accent glow, top-right. */}
      <div
        aria-hidden
        className={`pointer-events-none absolute -right-10 -top-10 h-36 w-36 rounded-full ${a.blob} blur-3xl`}
      />

      <div className="relative flex items-start gap-4">
        {icon != null && (
          <div
            aria-hidden
            className={`flex h-11 w-11 flex-shrink-0 items-center justify-center rounded-xl text-2xl ${a.tile}`}>
            {icon}
          </div>
        )}

        <div className="min-w-0 flex-1">
          <div className="flex items-start justify-between gap-3">
            <h1 className="font-title text-lg font-semibold text-content">{title}</h1>
            {action != null && <div className="flex-shrink-0">{action}</div>}
          </div>

          {description != null && (
            <p className="mt-1 max-w-2xl text-sm text-content-muted">{description}</p>
          )}

          {benefits != null && benefits.length > 0 && (
            <ul className="mt-3 flex flex-wrap gap-2">
              {benefits.map((b, i) => (
                <li
                  key={i}
                  className="inline-flex items-center gap-1.5 rounded-full border border-line bg-surface/70 px-2.5 py-1 text-xs font-medium text-content-secondary">
                  <span aria-hidden className="text-sm leading-none">
                    {b.icon}
                  </span>
                  {b.label}
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </section>
  );
}
