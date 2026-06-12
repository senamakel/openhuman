import type { ReactNode } from 'react';

export interface TwoPaneNavItem {
  value: string;
  label: string;
  icon?: ReactNode;
}

interface TwoPaneNavProps {
  items: TwoPaneNavItem[];
  selected: string;
  onSelect: (value: string) => void;
  /** Optional fixed header (title/subtitle) above the scrolling nav list. */
  header?: ReactNode;
  ariaLabel?: string;
}

/**
 * Vertical tab navigation for the sidebar pane of a {@link TwoPanelLayout} —
 * the left-rail counterpart to a horizontal PillTabBar. The list scrolls
 * independently below an optional fixed header.
 */
export default function TwoPaneNav({
  items,
  selected,
  onSelect,
  header,
  ariaLabel,
}: TwoPaneNavProps) {
  return (
    <nav aria-label={ariaLabel} className="flex h-full flex-col">
      {header && <div className="shrink-0 px-2 pb-1 pt-2">{header}</div>}
      <ul className="min-h-0 flex-1 overflow-y-auto p-1.5">
        {items.map(item => {
          const active = item.value === selected;
          return (
            <li key={item.value}>
              <button
                type="button"
                data-testid={`two-pane-nav-${item.value}`}
                aria-current={active ? 'page' : undefined}
                onClick={() => onSelect(item.value)}
                className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors ${
                  active
                    ? 'bg-stone-100 font-medium text-stone-900 dark:bg-neutral-800 dark:text-neutral-100'
                    : 'text-stone-600 hover:bg-stone-50 hover:text-stone-900 dark:text-neutral-300 dark:hover:bg-neutral-800/60 dark:hover:text-neutral-100'
                }`}>
                {item.icon && (
                  <span
                    className={`shrink-0 ${
                      active
                        ? 'text-primary-600 dark:text-primary-400'
                        : 'text-stone-400 dark:text-neutral-500'
                    }`}>
                    {item.icon}
                  </span>
                )}
                <span className="truncate">{item.label}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
