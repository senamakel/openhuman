import SidebarNav from './SidebarNav';
import { SidebarSlotOutlet } from './SidebarSlot';

/**
 * The root-shell sidebar: a single column split into two regions.
 *
 *   ┌──────────────┐
 *   │ SidebarNav    │  static, always-visible primary navigation
 *   ├──────────────┤
 *   │ SidebarSlot   │  dynamic, per-route content (scrolls)
 *   │  (Outlet)     │
 *   └──────────────┘
 *
 * Pages project content into the middle region with {@link SidebarContent}.
 * Background matches the previous in-page sidebar pane (white / neutral-900).
 */
export default function AppSidebar() {
  return (
    <div className="flex h-full min-h-0 flex-col bg-white dark:bg-neutral-900">
      <div className="flex-shrink-0">
        <SidebarNav />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto border-t border-stone-200/70 dark:border-neutral-800/70">
        {/* Flex column so routes that project more than one region (e.g. Chat's
            app rail above its thread list) can order them via Tailwind `order-*`. */}
        <SidebarSlotOutlet className="flex h-full flex-col" />
      </div>
    </div>
  );
}
