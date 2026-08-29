import { LuKeyboard, LuMegaphone, LuPanelLeftClose, LuSettings } from 'react-icons/lu';
import { useLocation, useNavigate } from 'react-router-dom';

import { registry } from '../../../lib/commands/registry';
import { useT } from '../../../lib/i18n/I18nContext';
import { Button, SidebarHeader as SidebarHeaderShell, Tooltip } from '../../ui';
import { useRootSidebar } from './RootShellLayout';

/**
 * Header footprint layered on `<Button variant="tertiary" iconOnly>`: 28px
 * square (no `size` maps to that) with the muted resting colour these utility
 * icons use. Focus ring, transition and hover fill come from the primitive.
 */
const ICON_BTN = 'h-7 w-7 flex-none rounded-md text-content-muted hover:text-content-secondary';

/**
 * Thin utility header at the top of the root sidebar: keyboard shortcuts,
 * Feedback, Settings, and collapse. Language is chosen from Settings, not here.
 *
 * Feedback moved up here from the sidebar footer row: it is a utility action
 * like the other three, not a destination the user returns to, and the footer
 * is now only the connectivity/version strip.
 */
export default function SidebarHeader() {
  const { t } = useT();
  const navigate = useNavigate();
  const location = useLocation();
  const { hide } = useRootSidebar();
  const feedbackActive = location.pathname === '/feedback';

  return (
    // The primitive's header slot supplies the px-3/pb-2/pt-3 band; this only
    // turns it into a right-aligned row. Right-aligned so the macOS traffic
    // lights (top-left, overlay title bar) sit in the empty left space — the
    // icons stay clear of the window controls and inline with them (no extra
    // top padding). `data-tauri-drag-region` lives directly on the primitive
    // (rather than a wrapping div in `AppSidebar`) so the header band is
    // draggable window chrome without an extra hand-rolled layout element.
    <SidebarHeaderShell data-tauri-drag-region className="flex-row items-center justify-end gap-1">
      <div className="flex items-center gap-0.5">
        {/* Keyboard shortcuts — one-click open of the help directory (also ? / ⌘/). */}
        <Tooltip label={t('shortcuts.title')}>
          <Button
            variant="tertiary"
            iconOnly
            onClick={() => registry.runAction('meta.keyboard-shortcuts')}
            className={ICON_BTN}
            analyticsId="sidebar-header-shortcuts"
            aria-label={t('shortcuts.title')}>
            <LuKeyboard className="h-4 w-4" />
          </Button>
        </Tooltip>

        {/* Share feedback — a utility action, so it sits with the other three
            rather than occupying a nav row. `aria-current` marks it while the
            route is open, matching how the nav rows signal the active page. */}
        <Tooltip label={t('nav.feedback')}>
          <Button
            variant="tertiary"
            iconOnly
            onClick={() => navigate('/feedback')}
            className={ICON_BTN}
            analyticsId="sidebar-header-feedback"
            data-walkthrough="tab-feedback"
            aria-current={feedbackActive ? 'page' : undefined}
            aria-label={t('nav.feedback')}>
            <LuMegaphone className="h-4 w-4" />
          </Button>
        </Tooltip>

        <Tooltip label={t('nav.settings')}>
          <Button
            variant="tertiary"
            iconOnly
            onClick={() => navigate('/settings')}
            className={ICON_BTN}
            analyticsId="sidebar-header-settings"
            aria-label={t('nav.settings')}>
            <LuSettings className="h-4 w-4" />
          </Button>
        </Tooltip>

        {/* Collapse the sidebar — sits on the right, next to Settings. */}
        <Tooltip label={t('chat.hideSidebar')}>
          <Button
            variant="tertiary"
            iconOnly
            onClick={hide}
            className={ICON_BTN}
            analyticsId="sidebar-header-collapse"
            aria-label={t('chat.hideSidebar')}>
            <LuPanelLeftClose className="h-4 w-4" />
          </Button>
        </Tooltip>
      </div>
    </SidebarHeaderShell>
  );
}
