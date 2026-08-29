import type { ReactNode } from 'react';

import { Button } from '../ui';

interface TwoPaneNavItem {
  value: string;
  label: string;
  icon?: ReactNode;
  /**
   * Accent an inactive row (e.g. Billing). Semantic, so it survives on the
   * label and icon; an active row still renders fully neutral on the fill.
   */
  highlight?: boolean;
  /** Overrides the derived `two-pane-nav-${value}` test id. */
  testId?: string;
}

interface TwoPaneNavGroup {
  /** Optional uppercase sub-header above the group's items. */
  label?: string;
  items: TwoPaneNavItem[];
  /** `data-testid` for the group container. */
  testId?: string;
}

interface TwoPaneNavProps {
  groups: TwoPaneNavGroup[];
  selected: string;
  onSelect: (value: string) => void;
  /** Optional fixed header (title/subtitle) above the scrolling nav list. */
  header?: ReactNode;
  /**
   * Optional content rendered inside the scroll region *below* the nav groups —
   * e.g. a separator + a live list of active sessions. Scrolls with the nav.
   */
  footer?: ReactNode;
  ariaLabel?: string;
  /** Walkthrough anchor for the nav element (Joyride target). */
  walkthroughId?: string;
}

/**
 * Vertical, grouped tab navigation for the sidebar pane of a
 * {@link TwoPanelLayout} — the left-rail counterpart to a horizontal
 * ChipTabs row, styled to match the settings sidebar (title header, labelled
 * sub-groups, icon + label rows). The list scrolls independently below the
 * optional fixed header.
 */
export default function TwoPaneNav({
  groups,
  selected,
  onSelect,
  header,
  footer,
  ariaLabel,
  walkthroughId,
}: TwoPaneNavProps) {
  return (
    <nav aria-label={ariaLabel} data-walkthrough={walkthroughId} className="flex h-full flex-col">
      {header && <div className="shrink-0 px-3 pb-1 pt-3">{header}</div>}
      {/* When there's no header, the list needs its own top padding so the first
          item doesn't collide with the pane's top edge. */}
      <div className={`min-h-0 flex-1 overflow-y-auto px-3 pb-2 ${header ? '' : 'pt-3'}`}>
        {groups.map((group, groupIndex) => (
          <div key={group.label ?? `__group-${groupIndex}`} data-testid={group.testId}>
            {group.label && (
              <div className="px-2 pb-0.5 pt-2.5">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-content-muted">
                  {group.label}
                </span>
              </div>
            )}
            <ul>
              {group.items.map(item => {
                const active = item.value === selected;
                return (
                  <li key={item.value}>
                    <Button
                      variant="tertiary"
                      data-testid={item.testId ?? `two-pane-nav-${item.value}`}
                      aria-current={active ? 'page' : undefined}
                      onClick={() => onSelect(item.value)}
                      // Same row spec as SidebarNav / SettingsSidebar /
                      // ThreadList: 15px, medium by default and semibold when
                      // selected, with an alpha fill that lifts against both the
                      // translucent chrome and an opaque pane.
                      className={`h-auto w-full justify-start rounded-md px-2.5 py-1.5 text-left text-[14px] ${
                        active
                          ? 'bg-primary-500 font-semibold text-content-inverted hover:bg-primary-500'
                          : item.highlight
                            ? 'font-normal text-primary-700 hover:bg-surface/40 dark:text-primary-300'
                            : 'font-normal text-content-muted hover:bg-surface/40 hover:text-content-secondary'
                      }`}>
                      <span
                        // `active` is tested first so a row that is both active
                        // and highlighted renders fully neutral — otherwise the
                        // label goes neutral while the icon keeps the accent.
                        className={`shrink-0 ${
                          active
                            ? 'text-content-inverted'
                            : item.highlight
                              ? 'text-primary-600 dark:text-primary-400'
                              : 'text-content-faint'
                        }`}>
                        {item.icon ?? null}
                      </span>
                      <span className="truncate">{item.label}</span>
                    </Button>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
        {footer}
      </div>
    </nav>
  );
}
