import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import NotificationsTabbedPanel from '../NotificationsTabbedPanel';

const navigateBack = vi.fn();

vi.mock('../../hooks/useSettingsNavigation', () => ({
  useSettingsNavigation: () => ({
    navigateBack,
    navigateToSettings: vi.fn(),
    navigateToTeamManagement: vi.fn(),
    breadcrumbs: [],
  }),
}));

vi.mock('../../components/SettingsHeader', () => ({
  default: ({ title }: { title: string }) => <div data-testid="settings-header">{title}</div>,
}));

// Stub the embedded child panels so we can assert on which one is rendered
// without dragging redux / webview services into this suite.
vi.mock('../NotificationsPanel', () => ({
  default: ({ embedded }: { embedded?: boolean }) => (
    <div data-testid="prefs-body" data-embedded={String(!!embedded)}>
      preferences body
    </div>
  ),
}));

vi.mock('../NotificationRoutingPanel', () => ({
  default: ({ embedded }: { embedded?: boolean }) => (
    <div data-testid="routing-body" data-embedded={String(!!embedded)}>
      routing body
    </div>
  ),
}));

// Spy on the live URL to verify the tab click rewrites the hash.
function LocationProbe() {
  const loc = useLocation();
  return <div data-testid="hash">{loc.hash}</div>;
}

function renderAt(initialEntry: string) {
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <Routes>
        <Route
          path="/settings/notifications"
          element={
            <>
              <NotificationsTabbedPanel />
              <LocationProbe />
            </>
          }
        />
      </Routes>
    </MemoryRouter>
  );
}

describe('NotificationsTabbedPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders the preferences tab by default and passes embedded=true', () => {
    renderAt('/settings/notifications');
    expect(screen.getByTestId('settings-header')).toHaveTextContent(/Notifications/);
    const body = screen.getByTestId('prefs-body');
    expect(body).toBeInTheDocument();
    expect(body).toHaveAttribute('data-embedded', 'true');
    expect(screen.queryByTestId('routing-body')).not.toBeInTheDocument();
  });

  it('lands on the routing tab when the URL hash is #routing', () => {
    renderAt('/settings/notifications#routing');
    expect(screen.getByTestId('routing-body')).toBeInTheDocument();
    expect(screen.queryByTestId('prefs-body')).not.toBeInTheDocument();
  });

  it('clicking the Routing tab updates the URL hash to #routing', () => {
    renderAt('/settings/notifications');
    fireEvent.click(screen.getByRole('tab', { name: /routing/i }));
    expect(screen.getByTestId('hash')).toHaveTextContent('#routing');
    expect(screen.getByTestId('routing-body')).toBeInTheDocument();
  });

  it('clicking Preferences from the routing tab clears the hash', () => {
    renderAt('/settings/notifications#routing');
    fireEvent.click(screen.getByRole('tab', { name: /preferences/i }));
    // empty hash means we navigated to the bare pathname
    expect(screen.getByTestId('hash')).toHaveTextContent('');
    expect(screen.getByTestId('prefs-body')).toBeInTheDocument();
  });

  it('marks the active tab with aria-selected=true', () => {
    renderAt('/settings/notifications#routing');
    const routingTab = screen.getByRole('tab', { name: /routing/i });
    const prefsTab = screen.getByRole('tab', { name: /preferences/i });
    expect(routingTab).toHaveAttribute('aria-selected', 'true');
    expect(prefsTab).toHaveAttribute('aria-selected', 'false');
  });
});
