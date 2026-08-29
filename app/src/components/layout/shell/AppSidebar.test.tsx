import { screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { renderWithProviders } from '../../../test/test-utils';
import { SidebarProvider } from '../../ui';
import AppSidebar from './AppSidebar';

/** `AppSidebar` reads `useSidebar()` — it must render inside a `<SidebarProvider>`. */
function renderAppSidebar(
  options?: Parameters<typeof renderWithProviders>[1],
  providerProps?: { open?: boolean }
) {
  return renderWithProviders(
    <SidebarProvider open={providerProps?.open ?? true}>
      <AppSidebar />
    </SidebarProvider>,
    options
  );
}

// Keep the mount light: the collapsed rail is the unit under test, not the
// header/nav children (SidebarHeader in particular needs the RootShellLayout
// context the harness doesn't provide). SidebarSlot is left real on purpose —
// the harness itself imports SidebarSlotProvider from it.
//
// The Rewards and Feedback footer rows that used to live here are gone:
// Rewards is a cloud-gated `NAV_TABS` destination (covered by
// `SidebarNav.test.tsx`) and Feedback is a header icon (covered by
// `SidebarHeader.test.tsx`). The footer is the status strip alone now.
vi.mock('../../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (k: string) => k }) }));
vi.mock('./SidebarHeader', () => ({ default: () => null }));
vi.mock('./SidebarNav', () => ({ default: () => null }));
vi.mock('./SidebarAppRail', () => ({ default: () => null }));
vi.mock('../../ConnectionIndicator', () => ({ default: () => null }));

// The `Sidebar` column stays mounted while collapsed (`collapsible="icon"`),
// so `AppSidebar` — not `RootShellLayout` — is what switches to the compact
// rail body. These render inside a collapsed `SidebarProvider` directly,
// bypassing the mocked SidebarHeader/SidebarNav above so the real collapsed
// branch (drag strip, reopen trigger, CollapsedNavRail) is under test.
describe('AppSidebar — collapsed rail', () => {
  beforeEach(() => vi.clearAllMocks());

  it('renders the reopen trigger and collapsed nav rail instead of the header/nav', () => {
    renderAppSidebar({ initialEntries: ['/chat'] }, { open: false });

    expect(screen.getByTestId('root-shell-reopen')).toBeInTheDocument();
    // The primary nav destinations still resolve via CollapsedNavRail.
    expect(screen.getByRole('button', { name: 'nav.chat' })).toBeInTheDocument();
  });

  it('reserves a draggable strip above the reopen trigger for the macOS traffic lights', () => {
    const { container } = renderAppSidebar({ initialEntries: ['/chat'] }, { open: false });
    expect(container.querySelector('[data-tauri-drag-region]')).toBeInTheDocument();
  });

  it('gives the reopen trigger the expected analytics id and label', () => {
    renderAppSidebar({ initialEntries: ['/chat'] }, { open: false });
    const reopen = screen.getByTestId('root-shell-reopen');
    expect(reopen).toHaveAttribute('data-analytics-id', 'root-shell-reopen-sidebar');
    expect(reopen).toHaveAttribute('aria-label', 'layout.showSidebar');
  });

  it('does not render the reopen trigger while expanded', () => {
    renderAppSidebar({ initialEntries: ['/chat'] }, { open: true });
    expect(screen.queryByTestId('root-shell-reopen')).not.toBeInTheDocument();
  });
});
