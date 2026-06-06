/**
 * Tests for BottomTabBar — verifies that:
 *  - the tab bar renders when the user has a session token and is on a non-hidden path
 *  - the walkthroughAttr mapping (line 222) is exercised by rendering the tabs
 *  - the tab bar is hidden on '/' and '/login' paths
 *
 * [#1123] Covers the walkthroughAttr object added for the Joyride walkthrough.
 */
import { configureStore } from '@reduxjs/toolkit';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { Provider } from 'react-redux';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import accountsReducer from '../../store/accountsSlice';
import agentProfileReducer, { setAgentProfilesFromResponse } from '../../store/agentProfileSlice';
import companionReducer from '../../store/companionSlice';
import notificationReducer from '../../store/notificationSlice';
import BottomTabBar from '../BottomTabBar';

// ── Module-level mocks ─────────────────────────────────────────────────────

vi.mock('../../providers/CoreStateProvider', () => ({ useCoreState: vi.fn() }));

const agentProfilesApiMock = vi.hoisted(() => ({
  list: vi.fn(),
  select: vi.fn(),
  upsert: vi.fn(),
  delete: vi.fn(),
}));

vi.mock('../../services/api/agentProfilesApi', () => ({ agentProfilesApi: agentProfilesApiMock }));

vi.mock('../../utils/config', async importOriginal => {
  const actual = await importOriginal<typeof import('../../utils/config')>();
  return { ...actual, APP_ENVIRONMENT: 'development' };
});

vi.mock('../../utils/accountsFullscreen', () => ({ isAccountsFullscreen: vi.fn(() => false) }));
vi.mock('../../services/analytics', () => ({ trackEvent: vi.fn() }));

// ── Helpers ────────────────────────────────────────────────────────────────

interface BuildStoreOpts {
  companionSessionActive?: boolean;
}

const testProfiles = {
  activeProfileId: 'planner',
  profiles: [
    {
      id: 'default',
      name: 'Orchestrator',
      description: 'Default agent',
      agentId: 'orchestrator',
      builtIn: true,
    },
    {
      id: 'planner',
      name: 'Planner',
      description: 'Plans multi-step work',
      agentId: 'planner',
      builtIn: true,
      avatarUrl: 'https://example.com/planner.png',
    },
    {
      id: 'research',
      name: 'Research',
      description: 'Finds and summarizes sources',
      agentId: 'research',
      builtIn: true,
    },
  ],
};

function buildStore(opts: BuildStoreOpts = {}) {
  const store = configureStore({
    reducer: {
      accounts: accountsReducer,
      notifications: notificationReducer,
      companion: companionReducer,
      agentProfiles: agentProfileReducer,
    },
  });
  store.dispatch(setAgentProfilesFromResponse(testProfiles));
  if (opts.companionSessionActive) {
    store.dispatch({
      type: 'companion/setSessionActive',
      payload: { active: true, sessionId: 'sess-test' },
    });
  }
  return store;
}

interface RenderOpts {
  hasToken?: boolean;
  companionSessionActive?: boolean;
  tokenValue?: string;
}

async function renderBottomTabBar(pathname = '/home', opts: RenderOpts | boolean = {}) {
  // Back-compat: previous callsites passed `hasToken` as the 2nd positional arg.
  const resolved: RenderOpts = typeof opts === 'boolean' ? { hasToken: opts } : opts;
  const hasToken = resolved.hasToken ?? true;
  const tokenValue = resolved.tokenValue ?? 'tok-test';
  const { useCoreState } = await import('../../providers/CoreStateProvider');
  vi.mocked(useCoreState).mockReturnValue({
    snapshot: {
      sessionToken: hasToken ? tokenValue : null,
      auth: { isAuthenticated: true, userId: 'u1', user: null, profileId: null },
      currentUser: null,
      onboardingCompleted: true,
      chatOnboardingCompleted: true,
      analyticsEnabled: false,
      localState: { encryptionKey: null, onboardingTasks: null, keyringConsent: null },
      keyringStatus: {
        available: true,
        failureReason: null,
        activeMode: 'os_keyring',
        backendName: 'os',
      },
      runtime: { screenIntelligence: null, localAi: null, autocomplete: null, service: null },
    },
    isBootstrapping: false,
    isReady: true,
    teams: [],
    teamMembersById: {},
    teamInvitesById: {},
    setOnboardingCompletedFlag: vi.fn(),
    setOnboardingTasks: vi.fn(),
    refreshSnapshot: vi.fn(),
  } as never);

  const store = buildStore({ companionSessionActive: resolved.companionSessionActive });
  return render(
    <Provider store={store}>
      <MemoryRouter initialEntries={[pathname]}>
        <BottomTabBar />
      </MemoryRouter>
    </Provider>
  );
}

// ── Tests ──────────────────────────────────────────────────────────────────

describe('BottomTabBar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    agentProfilesApiMock.select.mockResolvedValue(testProfiles);
  });

  // [#1123] Covers line 222 — walkthroughAttr object created per-tab inside .map()
  it('renders navigation tabs with data-walkthrough attributes when session is active', async () => {
    await renderBottomTabBar('/home');

    // The Home tab is always visible and has no walkthrough attr (not in the map)
    expect(screen.getByRole('button', { name: 'Home' })).toBeInTheDocument();

    // Chat tab has data-walkthrough="tab-chat" (from walkthroughAttr map)
    const chatBtn = screen.getByRole('button', { name: 'Chat' });
    expect(chatBtn).toBeInTheDocument();
    expect(chatBtn).toHaveAttribute('data-walkthrough', 'tab-chat');
  });

  it('renders Settings tab with data-walkthrough="tab-settings"', async () => {
    await renderBottomTabBar('/home');
    const settingsBtn = screen.getByRole('button', { name: 'Settings' });
    expect(settingsBtn).toHaveAttribute('data-walkthrough', 'tab-settings');
  });

  it('returns null when there is no session token', async () => {
    const { container } = await renderBottomTabBar('/home', { hasToken: false });
    expect(container.firstChild).toBeNull();
  });

  it('still shows the Rewards tab for local sessions', async () => {
    await renderBottomTabBar('/home', { tokenValue: 'header.payload.local' });
    expect(screen.getByRole('button', { name: 'Rewards' })).toBeInTheDocument();
  });

  it('renders the pulsing companion dot on the Settings tab when a session is active', async () => {
    const { container } = await renderBottomTabBar('/home', { companionSessionActive: true });
    const settingsBtn = screen.getByRole('button', { name: 'Settings' });
    const dot = settingsBtn.querySelector('.animate-pulse.bg-blue-500');
    expect(dot).not.toBeNull();
    // And not on a non-Settings tab.
    const homeBtn = screen.getByRole('button', { name: 'Home' });
    expect(homeBtn.querySelector('.animate-pulse.bg-blue-500')).toBeNull();
    void container;
  });

  it('returns null on the "/" path even with a session token', async () => {
    const { container } = await renderBottomTabBar('/');
    expect(container.firstChild).toBeNull();
  });

  it('uses pointer-events-none on the full-width shell so side areas do not block clicks', async () => {
    const { container } = await renderBottomTabBar('/home');
    const shell = container.firstElementChild;
    expect(shell).toHaveClass('pointer-events-none');
    expect(shell?.querySelector('nav')).toHaveClass('pointer-events-auto');
  });

  it('tracks tab changes when a different tab is clicked', async () => {
    const { trackEvent } = await import('../../services/analytics');
    await renderBottomTabBar('/home');

    fireEvent.click(screen.getByRole('button', { name: 'Chat' }));

    expect(trackEvent).toHaveBeenCalledWith('tab_bar_change', {
      from_tab: 'home',
      to_tab: 'chat',
      from_path: '/home',
      to_path: '/chat',
    });
  });

  it('does not track when the active tab is clicked again', async () => {
    const { trackEvent } = await import('../../services/analytics');
    await renderBottomTabBar('/home');

    fireEvent.click(screen.getByRole('button', { name: 'Home' }));

    expect(trackEvent).not.toHaveBeenCalled();
  });

  it('renders an avatar-only agent profile switcher with the active profile', async () => {
    await renderBottomTabBar('/home');

    const switcher = screen.getByRole('button', { name: 'Switch agent profile: Planner' });
    expect(switcher).toHaveAttribute('title', 'Planner');
    expect(switcher.querySelector('img')).toHaveAttribute('src', 'https://example.com/planner.png');

    fireEvent.click(switcher);

    expect(screen.getByRole('menu', { name: 'Agent profiles' })).toBeInTheDocument();
    expect(screen.getByRole('menuitemradio', { name: /Planner/ })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    expect(screen.getByRole('menuitemradio', { name: /Research/ })).toBeInTheDocument();
  });

  it('switches the active agent profile from the taskbar menu', async () => {
    const { trackEvent } = await import('../../services/analytics');
    agentProfilesApiMock.select.mockResolvedValueOnce({
      ...testProfiles,
      activeProfileId: 'research',
    });
    await renderBottomTabBar('/home');

    fireEvent.click(screen.getByRole('button', { name: 'Switch agent profile: Planner' }));
    fireEvent.click(screen.getByRole('menuitemradio', { name: /Research/ }));

    await waitFor(() => expect(agentProfilesApiMock.select).toHaveBeenCalledWith('research'));
    expect(trackEvent).toHaveBeenCalledWith('agent_profile_switch', {
      from_profile_id: 'planner',
      to_profile_id: 'research',
      from_path: '/home',
    });
  });
});
