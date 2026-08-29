import { fireEvent, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { registry } from '../../../lib/commands/registry';
import { renderWithProviders } from '../../../test/test-utils';
import SidebarHeader from './SidebarHeader';

const mockNavigate = vi.fn();
const mockHide = vi.fn();

vi.mock('react-router-dom', async importOriginal => {
  const actual = await importOriginal<typeof import('react-router-dom')>();
  return { ...actual, useNavigate: () => mockNavigate };
});
vi.mock('./RootShellLayout', () => ({ useRootSidebar: () => ({ hide: mockHide }) }));
// Return i18n keys verbatim so queries don't depend on locale.
vi.mock('../../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (k: string) => k }) }));

describe('SidebarHeader', () => {
  beforeEach(() => vi.clearAllMocks());

  it('renders Keyboard Shortcuts, Feedback, Settings, and Collapse buttons', () => {
    renderWithProviders(<SidebarHeader />, { initialEntries: ['/home'] });
    expect(screen.getByRole('button', { name: 'shortcuts.title' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'nav.feedback' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'nav.settings' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'chat.hideSidebar' })).toBeInTheDocument();
    // The wallet shortcut was removed long ago; Home followed it, since the
    // primary nav directly below already carries Chat.
    expect(screen.queryByRole('button', { name: 'nav.wallet' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'nav.home' })).not.toBeInTheDocument();
  });

  it('shortcuts button opens the keyboard-shortcuts help directory', () => {
    const runAction = vi.spyOn(registry, 'runAction').mockReturnValue(true);
    renderWithProviders(<SidebarHeader />, { initialEntries: ['/home'] });
    fireEvent.click(screen.getByRole('button', { name: 'shortcuts.title' }));
    expect(runAction).toHaveBeenCalledWith('meta.keyboard-shortcuts');
    runAction.mockRestore();
  });

  it('shortcuts button has correct data-analytics-id', () => {
    renderWithProviders(<SidebarHeader />, { initialEntries: ['/home'] });
    expect(screen.getByRole('button', { name: 'shortcuts.title' })).toHaveAttribute(
      'data-analytics-id',
      'sidebar-header-shortcuts'
    );
  });

  it('shortcuts button has matching aria-label and title', () => {
    renderWithProviders(<SidebarHeader />, { initialEntries: ['/home'] });
    const btn = screen.getByRole('button', { name: 'shortcuts.title' });
    expect(btn).toHaveAttribute('aria-label', 'shortcuts.title');
    // The styled <Tooltip> wrapper re-applies a native `title` fallback so the
    // label still surfaces if the portal pill is occluded by a CEF webview.
    expect(btn).toHaveAttribute('title', 'shortcuts.title');
  });

  it('settings button navigates to /settings', () => {
    renderWithProviders(<SidebarHeader />, { initialEntries: ['/home'] });
    fireEvent.click(screen.getByRole('button', { name: 'nav.settings' }));
    expect(mockNavigate).toHaveBeenCalledWith('/settings');
  });

  // Feedback moved here from the sidebar footer row: it is a utility action
  // like the other three, not a destination the user returns to.
  it('feedback button navigates to /feedback', () => {
    renderWithProviders(<SidebarHeader />, { initialEntries: ['/home'] });
    fireEvent.click(screen.getByRole('button', { name: 'nav.feedback' }));
    expect(mockNavigate).toHaveBeenCalledWith('/feedback');
  });

  it('marks the feedback button current while the route is open', () => {
    renderWithProviders(<SidebarHeader />, { initialEntries: ['/feedback'] });
    expect(screen.getByRole('button', { name: 'nav.feedback' })).toHaveAttribute(
      'aria-current',
      'page'
    );
  });

  it('Collapse button calls hide()', () => {
    renderWithProviders(<SidebarHeader />, { initialEntries: ['/home'] });
    fireEvent.click(screen.getByRole('button', { name: 'chat.hideSidebar' }));
    expect(mockHide).toHaveBeenCalledTimes(1);
  });
});
