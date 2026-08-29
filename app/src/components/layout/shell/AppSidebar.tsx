import debugFactory from 'debug';
import { useEffect } from 'react';
import { LuPanelLeftOpen } from 'react-icons/lu';

import { useT } from '../../../lib/i18n/I18nContext';
import { APP_VERSION } from '../../../utils/config';
import ConnectionIndicator from '../../ConnectionIndicator';
import {
  SidebarFooter,
  SidebarContent as SidebarScrollRegion,
  SidebarTrigger,
  Tooltip,
  useSidebar,
} from '../../ui';
import CollapsedNavRail from './CollapsedNavRail';
import SidebarHeader from './SidebarHeader';
import SidebarNav from './SidebarNav';
import { SidebarSlotOutlet } from './SidebarSlot';

const log = debugFactory('sidebar');

/**
 * The root-shell sidebar. Mounted as the sole child of `RootShellLayout`'s
 * `<Sidebar collapsible="icon">` column, so it renders one of two bodies
 * depending on that primitive's own `useSidebar()` state — the column itself
 * never unmounts, only narrows, so this component is what actually decides
 * what the collapsed state looks like:
 *
 * **Expanded**, split top-to-bottom:
 *
 *   ┌──────────────┐
 *   │ SidebarHeader │  utility row (collapse / settings / language)
 *   ├──────────────┤
 *   │ SidebarNav    │  static primary navigation
 *   │ SidebarSlot   │  dynamic, per-route content (scrolls)
 *   │  (Outlet)     │
 *   ├──────────────┤
 *   │ Rewards/Fdbk  │  account affordances
 *   ├──────────────┤
 *   │ beta footer   │  app-wide build/version line
 *   └──────────────┘
 *
 * Pages project content into the slot region with {@link SidebarContent}.
 * Background matches the previous in-page sidebar pane (white / neutral-900).
 *
 * **Collapsed**: a draggable strip (clears the macOS traffic lights), a
 * reopen trigger, and {@link CollapsedNavRail}'s icon-only nav — formerly a
 * sibling `<div>` rendered by `RootShellLayout` outside the (unmounted)
 * `Sidebar` column; now the column's own body while narrow. See
 * `RootShellLayout`'s `collapsible="icon"` comment for why that's safe.
 */
export default function AppSidebar() {
  const { t } = useT();
  const { state: sidebarState } = useSidebar();
  const collapsed = sidebarState === 'collapsed';

  useEffect(() => {
    log('sidebar body: %s', collapsed ? 'collapsed rail' : 'expanded');
  }, [collapsed]);

  if (collapsed) {
    return (
      // Occupies the same {@link SIDEBAR_ICON_WIDTH} column as the expanded
      // body below — no fill of its own, chrome shows through (see the
      // expanded-branch comment for why). `items-center` centers the
      // fixed-size trigger/rail buttons in the narrow column.
      <div className="flex h-full min-h-0 flex-col items-center gap-0.5">
        {/* macOS overlay title bar (titleBarStyle: Overlay) floats the traffic
            lights over the top-left. The expanded SidebarHeader dodges them by
            right-aligning, but this narrow rail can't — so reserve a draggable
            strip the height of the window controls and start the rail below
            it, clear of the lights. */}
        <div className="h-7 w-full flex-none" data-tauri-drag-region />
        <Tooltip label={t('layout.showSidebar')}>
          {/* The primitive's own trigger, so reopening goes through the same
              controlled `onOpenChange` `RootShellLayout` drives every other
              visibility change through. 32px square: no primitive size maps
              to that, so the footprint is overridden while the focus
              ring/transition come from the trigger. */}
          <SidebarTrigger
            data-testid="root-shell-reopen"
            data-analytics-id="root-shell-reopen-sidebar"
            aria-label={t('layout.showSidebar')}
            className="h-8 w-8 rounded-lg">
            <LuPanelLeftOpen className="h-4 w-4" />
          </SidebarTrigger>
        </Tooltip>
        {/* Keep the primary nav reachable while collapsed: an icon-only rail.
            Kept as its own component rather than folded into `SidebarNav` —
            it covers more ground than that file's `NAV_TABS` loop (it also
            stands in for `SidebarHeader`'s Home/shortcuts/settings actions,
            none of which are nav tabs), so a shared render path would mean
            `SidebarNav` growing a second, unrelated responsibility instead of
            just adapting its own rows to icon width. */}
        <div className="mt-1 w-full pt-1">
          <CollapsedNavRail />
        </div>
      </div>
    );
  }

  return (
    // Sits directly on the window chrome with no fill of its own, so the
    // sidebar and the frame around the content card are one continuous surface.
    // The legibility scrim lives on the shell root ({@link RootShellLayout}) and
    // deliberately NOT here — scrimming only this column would tint it
    // differently from the chrome beside the card, which is the seam the
    // two-layer look exists to remove. Regions below are separated by spacing
    // alone; the hairline seams the old opaque panel needed would draw lines
    // across the chrome.
    <div className="flex h-full min-h-0 flex-col">
      <SidebarHeader />
      <SidebarNav />
      <SidebarScrollRegion className="gap-0">
        {/* Flex column so routes that project more than one region can order
            them via Tailwind `order-*`. */}
        <SidebarSlotOutlet className="flex h-full flex-col" />
      </SidebarScrollRegion>
      <SidebarFooter>
        {/* App-wide footer: connectivity status + build/version, pinned to the
            bottom of the sidebar. Rewards and Feedback were rows here once;
            Rewards is a primary `NAV_TABS` destination now and Feedback is a
            header icon beside the keyboard shortcut, so the footer is the
            status strip alone. */}
        <div className="flex flex-wrap items-center justify-center gap-x-2 gap-y-0.5">
          <ConnectionIndicator />
          &middot;
          <span className="text-[10px] text-content-faint">
            {t('settings.betaBuild').replace('{version}', APP_VERSION)}
          </span>
        </div>
      </SidebarFooter>
    </div>
  );
}
