/**
 * Tests for SearchPanel: the multi-provider web search settings.
 *
 * Covers the data-driven sections rendered from `config_get_search_settings`:
 *  - the global on/off switch,
 *  - provider cards (enable switch, route choice, key editor, SearXNG URL,
 *    status badges, deep-research note),
 *  - the per-role provider order (reorder, remove, add, reset, serving hint),
 *  - the local-session state where managed routes are unavailable,
 *  - the Advanced presentation toggle,
 *  - the Allowed websites section (Allow all / Custom / Block all).
 */
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, test, vi } from 'vitest';

import { renderWithProviders } from '../../../test/test-utils';
import SearchPanel from './SearchPanel';

// ---------------------------------------------------------------------------
// Hoisted mocks
// ---------------------------------------------------------------------------
const hoisted = vi.hoisted(() => ({
  getSearchSettings: vi.fn(),
  updateSearchSettings: vi.fn(),
  localSession: false,
}));

vi.mock('../../../utils/tauriCommands/config', () => ({
  openhumanGetSearchSettings: (...a: unknown[]) => hoisted.getSearchSettings(...a),
  openhumanUpdateSearchSettings: (...a: unknown[]) => hoisted.updateSearchSettings(...a),
}));

// Identity translator so we can query by the stable i18n keys.
vi.mock('../../../lib/i18n/I18nContext', () => ({ useT: () => ({ t: (key: string) => key }) }));

vi.mock('../hooks/useSettingsNavigation', () => ({
  useSettingsNavigation: () => ({ navigateBack: vi.fn(), breadcrumbs: [] }),
}));

vi.mock('../../../utils/localSession', () => ({ isLocalSessionToken: () => hoisted.localSession }));

// ---------------------------------------------------------------------------
// Fixtures (shape of the core's config_get_search_settings response)
// ---------------------------------------------------------------------------
type Provider = Record<string, unknown> & { id: string };

function provider(id: string, overrides: Record<string, unknown> = {}): Provider {
  const base: Record<string, Provider> = {
    exa: {
      id: 'exa',
      label: 'Exa',
      enabled: true,
      route: 'managed',
      routes: ['managed', 'direct'],
      managed_available: true,
      key_configured: false,
      takes_key: true,
      usable: true,
      status: 'ready',
      roles: ['search', 'answer', 'contents'],
      docs_url: 'https://dashboard.exa.ai/api-keys',
    },
    gemini: {
      id: 'gemini',
      label: 'Gemini',
      enabled: true,
      route: 'managed',
      routes: ['managed', 'direct'],
      managed_available: true,
      key_configured: false,
      takes_key: true,
      usable: true,
      status: 'ready',
      roles: ['answer'],
      docs_url: 'https://aistudio.google.com/apikey',
      deep_research_available: false,
    },
    brave: {
      id: 'brave',
      label: 'Brave',
      enabled: false,
      route: 'direct',
      routes: ['direct'],
      managed_available: true,
      key_configured: false,
      takes_key: true,
      usable: false,
      status: 'disabled',
      roles: ['search'],
      docs_url: 'https://brave.com/search/api/',
    },
    searxng: {
      id: 'searxng',
      label: 'SearXNG',
      enabled: false,
      route: 'direct',
      routes: ['direct'],
      managed_available: true,
      key_configured: true,
      takes_key: false,
      usable: false,
      status: 'disabled',
      roles: ['search'],
      docs_url: 'https://docs.searxng.org/',
      base_url: 'http://localhost:8080',
    },
  };
  return { ...base[id], ...overrides };
}

function settings(overrides: Record<string, unknown> = {}) {
  return {
    enabled: true,
    presentation: 'roles',
    presentation_provider: null,
    max_results: 5,
    timeout_secs: 15,
    managed_available: true,
    providers: [provider('exa'), provider('gemini'), provider('brave'), provider('searxng')],
    roles: { search: ['exa', 'brave', 'searxng'], answer: ['gemini', 'exa'], contents: ['exa'] },
    effective_roles: { search: ['exa'], answer: ['gemini', 'exa'], contents: ['exa'] },
    allowed_domains: ['reuters.com'],
    allow_all: false,
    ...overrides,
  };
}

const PLACEHOLDER = 'settings.search.allowedSitesPlaceholder';
const ALLOW_ALL = 'settings.search.accessAllowAll';
const CUSTOM = 'settings.search.accessCustom';
const BLOCK_ALL = 'settings.search.accessBlockAll';

const radio = (name: string) => screen.getByRole('radio', { name });
const card = (id: string) => screen.getByTestId(`search-provider-${id}`);
const roleRow = (role: string) => screen.getByTestId(`search-role-${role}`);

async function renderPanel() {
  renderWithProviders(<SearchPanel embedded />);
  await screen.findByTestId('search-provider-exa');
}

beforeEach(() => {
  hoisted.localSession = false;
  hoisted.getSearchSettings.mockReset();
  hoisted.updateSearchSettings.mockReset();
  hoisted.getSearchSettings.mockResolvedValue({ result: settings() });
  hoisted.updateSearchSettings.mockResolvedValue({ result: settings() });
});

describe('SearchPanel — search on/off', () => {
  test('the global switch persists enabled: false', async () => {
    await renderPanel();
    const toggle = screen.getByTestId('search-enabled-toggle');
    expect(toggle).toHaveAttribute('aria-checked', 'true');

    fireEvent.click(toggle);

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({ enabled: false })
    );
  });

  test('the returned settings replace local state', async () => {
    hoisted.updateSearchSettings.mockResolvedValue({ result: settings({ enabled: false }) });
    await renderPanel();

    fireEvent.click(screen.getByTestId('search-enabled-toggle'));

    await waitFor(() =>
      expect(screen.getByTestId('search-enabled-toggle')).toHaveAttribute('aria-checked', 'false')
    );
    expect(screen.getByText('settings.search.statusSaved')).toBeInTheDocument();
  });

  test('an RPC error shows on the status line and keeps the previous state', async () => {
    hoisted.updateSearchSettings.mockRejectedValue(new Error('unsupported route'));
    await renderPanel();

    fireEvent.click(screen.getByTestId('search-enabled-toggle'));

    expect(await screen.findByText(/unsupported route/)).toBeInTheDocument();
    expect(screen.getByTestId('search-enabled-toggle')).toHaveAttribute('aria-checked', 'true');
  });

  test('a failed load shows the error', async () => {
    hoisted.getSearchSettings.mockRejectedValue(new Error('core offline'));
    renderWithProviders(<SearchPanel embedded />);

    expect(await screen.findByText(/core offline/)).toBeInTheDocument();
  });
});

describe('SearchPanel — providers', () => {
  test('renders one card per provider with a status badge from `status`', async () => {
    hoisted.getSearchSettings.mockResolvedValue({
      result: settings({
        providers: [
          provider('exa'),
          provider('gemini', { status: 'sign_in_required', usable: false }),
          provider('brave', { enabled: true, status: 'needs_key' }),
          provider('searxng'),
        ],
      }),
    });
    await renderPanel();

    const badge = (id: string) => screen.getByTestId(`search-provider-${id}-status`);
    expect(badge('exa')).toHaveTextContent('settings.search.statusReady');
    expect(badge('gemini')).toHaveTextContent('settings.search.statusSignInRequired');
    expect(badge('brave')).toHaveTextContent('settings.search.statusNeedsKey');
    expect(badge('searxng')).toHaveTextContent('settings.search.statusOff');
  });

  test('toggling a provider persists its enabled flag', async () => {
    await renderPanel();
    const toggle = screen.getByTestId('search-provider-brave-toggle');
    expect(toggle).toHaveAttribute('aria-checked', 'false');

    fireEvent.click(toggle);

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({
        providers: { brave: { enabled: true } },
      })
    );
  });

  test('the route control only shows for providers with more than one route', async () => {
    await renderPanel();

    expect(screen.getByTestId('search-provider-exa-route-managed')).toBeInTheDocument();
    expect(screen.getByTestId('search-provider-exa-route-direct')).toBeInTheDocument();
    expect(screen.queryByTestId('search-provider-brave-route-direct')).toBeNull();
  });

  test('switching route persists it', async () => {
    await renderPanel();
    expect(screen.getByTestId('search-provider-exa-route-managed')).toHaveAttribute(
      'aria-checked',
      'true'
    );

    fireEvent.click(screen.getByTestId('search-provider-exa-route-direct'));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({
        providers: { exa: { route: 'direct' } },
      })
    );
  });

  test('a managed-route provider without deep research shows no key editor', async () => {
    await renderPanel();

    expect(within(card('exa')).queryByTestId('search-provider-exa-key')).toBeNull();
  });

  test('Gemini keeps its key editor on the managed route and hints at deep research', async () => {
    await renderPanel();

    expect(within(card('gemini')).getByTestId('search-provider-gemini-key')).toBeInTheDocument();
    expect(screen.getByTestId('search-provider-gemini-deep-research')).toHaveTextContent(
      'settings.search.deepResearchHint'
    );
  });

  test('Gemini notes deep research once it is available', async () => {
    hoisted.getSearchSettings.mockResolvedValue({
      result: settings({
        providers: [
          provider('exa'),
          provider('gemini', { key_configured: true, deep_research_available: true }),
        ],
      }),
    });
    await renderPanel();

    expect(screen.getByTestId('search-provider-gemini-deep-research')).toHaveTextContent(
      'settings.search.deepResearchAvailable'
    );
  });

  test('saving a key sends it for that provider and clears the draft', async () => {
    await renderPanel();
    const editor = within(screen.getByTestId('search-provider-brave-key'));
    const input = editor.getByPlaceholderText('settings.search.placeholderKey') as HTMLInputElement;
    expect(input.type).toBe('password');

    fireEvent.click(editor.getByText('settings.search.show'));
    expect(input.type).toBe('text');
    fireEvent.change(input, { target: { value: 'brave-test-key' } });
    fireEvent.click(editor.getByText('settings.search.save'));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({
        providers: { brave: { api_key: 'brave-test-key' } },
      })
    );
    await waitFor(() => expect(input.value).toBe(''));
  });

  test('a stored key can be cleared', async () => {
    hoisted.getSearchSettings.mockResolvedValue({
      result: settings({
        providers: [provider('exa'), provider('brave', { key_configured: true })],
      }),
    });
    await renderPanel();
    const editor = within(screen.getByTestId('search-provider-brave-key'));
    expect(editor.getByPlaceholderText('settings.search.placeholderStored')).toBeInTheDocument();

    fireEvent.click(editor.getByText('settings.search.clear'));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({
        providers: { brave: { api_key: '' } },
      })
    );
  });

  test('the key editor links to the provider docs_url', async () => {
    await renderPanel();

    const link = within(screen.getByTestId('search-provider-brave-key')).getByRole('link');
    expect(link).toHaveAttribute('href', 'https://brave.com/search/api/');
  });

  test('SearXNG shows its instance URL field and saves base_url', async () => {
    await renderPanel();
    const input = screen.getByTestId('search-provider-searxng-base-url') as HTMLInputElement;
    expect(input.value).toBe('http://localhost:8080');
    expect(within(card('searxng')).queryByTestId('search-provider-searxng-key')).toBeNull();

    fireEvent.change(input, { target: { value: 'https://search.example.org ' } });
    fireEvent.click(within(card('searxng')).getByText('settings.search.baseUrlSave'));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({
        providers: { searxng: { base_url: 'https://search.example.org' } },
      })
    );
  });
});

describe('SearchPanel — local session', () => {
  test('managed routes are disabled and the local-session hint shows', async () => {
    hoisted.localSession = true;
    hoisted.getSearchSettings.mockResolvedValue({
      result: settings({
        managed_available: false,
        providers: [
          provider('exa', { managed_available: false, status: 'sign_in_required', usable: false }),
          provider('gemini', { managed_available: false, status: 'sign_in_required' }),
        ],
        effective_roles: { search: [], answer: [], contents: [] },
      }),
    });
    await renderPanel();

    expect(screen.getByText('settings.search.localManagedUnavailable')).toBeInTheDocument();
    expect(screen.getByTestId('search-provider-exa-route-managed')).toBeDisabled();
    expect(screen.getByTestId('search-provider-exa-route-direct')).not.toBeDisabled();
    expect(screen.getByTestId('search-provider-exa-status')).toHaveTextContent(
      'settings.search.statusSignInRequired'
    );
  });

  test('a signed-in session shows no local-session hint', async () => {
    await renderPanel();

    expect(screen.queryByText('settings.search.localManagedUnavailable')).toBeNull();
  });
});

describe('SearchPanel — roles', () => {
  test('each role lists its ordered providers and who serves it', async () => {
    await renderPanel();

    const answer = within(roleRow('answer'));
    expect(answer.getByTestId('search-role-answer-provider-gemini')).toHaveAttribute(
      'data-serving',
      'true'
    );
    expect(answer.getByTestId('search-role-answer-provider-exa')).not.toHaveAttribute(
      'data-serving'
    );
    expect(answer.getByTestId('search-role-answer-serving')).toHaveTextContent(
      'settings.search.roleServedBy'
    );
    // Brave cannot serve the answer role, so it is neither listed nor addable.
    expect(answer.queryByTestId('search-role-answer-provider-brave')).toBeNull();
    expect(answer.queryByTestId('search-role-answer-add-brave')).toBeNull();
  });

  test('a role with no usable provider says so', async () => {
    hoisted.getSearchSettings.mockResolvedValue({
      result: settings({ effective_roles: { search: ['exa'], answer: ['gemini'], contents: [] } }),
    });
    await renderPanel();

    expect(screen.getByTestId('search-role-contents-serving')).toHaveTextContent(
      'settings.search.roleNoProvider'
    );
  });

  test('moving a provider down saves the new order', async () => {
    await renderPanel();
    const row = within(screen.getByTestId('search-role-answer-provider-gemini'));

    fireEvent.click(row.getByLabelText('settings.search.roleMoveDown'));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({
        roles: { answer: ['exa', 'gemini'] },
      })
    );
  });

  test('the first provider cannot move up and the last cannot move down', async () => {
    await renderPanel();

    expect(
      within(screen.getByTestId('search-role-answer-provider-gemini')).getByLabelText(
        'settings.search.roleMoveUp'
      )
    ).toBeDisabled();
    expect(
      within(screen.getByTestId('search-role-answer-provider-exa')).getByLabelText(
        'settings.search.roleMoveDown'
      )
    ).toBeDisabled();
  });

  test('removing a fallback saves the shorter order; the last one cannot be removed', async () => {
    await renderPanel();

    expect(
      within(screen.getByTestId('search-role-contents-provider-exa')).getByLabelText(
        'settings.search.roleRemove'
      )
    ).toBeDisabled();

    fireEvent.click(
      within(screen.getByTestId('search-role-answer-provider-exa')).getByLabelText(
        'settings.search.roleRemove'
      )
    );

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({ roles: { answer: ['gemini'] } })
    );
  });

  test('a removed provider can be added back to the end', async () => {
    hoisted.getSearchSettings.mockResolvedValue({
      result: settings({
        roles: { search: ['exa'], answer: ['gemini', 'exa'], contents: ['exa'] },
      }),
    });
    await renderPanel();

    fireEvent.click(screen.getByTestId('search-role-search-add-brave'));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({
        roles: { search: ['exa', 'brave'] },
      })
    );
  });

  test('reset sends an empty order to restore the default', async () => {
    await renderPanel();

    fireEvent.click(screen.getByTestId('search-role-search-reset'));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({ roles: { search: [] } })
    );
  });
});

describe('SearchPanel — advanced', () => {
  test('exposing provider tools switches presentation to all_tools and back', async () => {
    await renderPanel();

    fireEvent.click(screen.getByText('settings.search.advancedTitle'));
    const toggle = await screen.findByTestId('search-presentation-toggle');
    expect(toggle).toHaveAttribute('aria-checked', 'false');
    fireEvent.click(toggle);

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({ presentation: 'all_tools' })
    );
  });

  test('turning it off restores role presentation', async () => {
    hoisted.getSearchSettings.mockResolvedValue({
      result: settings({ presentation: 'all_tools' }),
    });
    await renderPanel();

    fireEvent.click(screen.getByText('settings.search.advancedTitle'));
    fireEvent.click(await screen.findByTestId('search-presentation-toggle'));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({ presentation: 'roles' })
    );
  });
});

describe('SearchPanel — allowed websites', () => {
  test('explicit host list → starts in Custom mode with the editor populated', async () => {
    renderWithProviders(<SearchPanel embedded />);
    await waitFor(() => {
      const ta = screen.getByPlaceholderText(PLACEHOLDER) as HTMLTextAreaElement;
      expect(ta.value).toBe('reuters.com');
    });
    expect(radio(CUSTOM)).toHaveAttribute('aria-checked', 'true');
    expect(radio(ALLOW_ALL)).toHaveAttribute('aria-checked', 'false');
  });

  test('selecting "Allow all" persists allow_all: true and hides the editor', async () => {
    renderWithProviders(<SearchPanel embedded />);
    await screen.findByPlaceholderText(PLACEHOLDER);

    fireEvent.click(radio(ALLOW_ALL));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({ allow_all: true })
    );
    expect(screen.queryByPlaceholderText(PLACEHOLDER)).toBeNull();
  });

  test('selecting "Block all" persists an empty allowlist and hides the editor', async () => {
    renderWithProviders(<SearchPanel embedded />);
    await screen.findByPlaceholderText(PLACEHOLDER);

    fireEvent.click(radio(BLOCK_ALL));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({
        allowed_domains: [],
        allow_all: false,
      })
    );
    expect(screen.queryByPlaceholderText(PLACEHOLDER)).toBeNull();
  });

  test('Custom: saving an edited host list persists allowed_domains + allow_all: false', async () => {
    renderWithProviders(<SearchPanel embedded />);
    const textarea = await screen.findByPlaceholderText(PLACEHOLDER);

    fireEvent.change(textarea, { target: { value: 'github.com\n  apnews.com  \n\n' } });
    fireEvent.click(screen.getByText('settings.search.allowedSitesSave'));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({
        allowed_domains: ['github.com', 'apnews.com'],
        allow_all: false,
      })
    );
  });

  test('Custom: pasted URLs are normalized to bare hosts before persisting', async () => {
    renderWithProviders(<SearchPanel embedded />);
    const textarea = await screen.findByPlaceholderText(PLACEHOLDER);

    fireEvent.change(textarea, {
      target: { value: 'https://reuters.com/markets\nhttp://apnews.com/\ngithub.com' },
    });
    fireEvent.click(screen.getByText('settings.search.allowedSitesSave'));

    await waitFor(() =>
      expect(hoisted.updateSearchSettings).toHaveBeenCalledWith({
        allowed_domains: ['reuters.com', 'apnews.com', 'github.com'],
        allow_all: false,
      })
    );
  });

  test('allow_all settings → starts in Allow-all mode with no editor', async () => {
    hoisted.getSearchSettings.mockResolvedValue({
      result: settings({ allowed_domains: ['*'], allow_all: true }),
    });
    renderWithProviders(<SearchPanel embedded />);

    await waitFor(() => expect(radio(ALLOW_ALL)).toHaveAttribute('aria-checked', 'true'));
    expect(screen.queryByPlaceholderText(PLACEHOLDER)).toBeNull();
  });

  test('empty allowlist → starts in Block-all mode with no editor', async () => {
    hoisted.getSearchSettings.mockResolvedValue({
      result: settings({ allowed_domains: [], allow_all: false }),
    });
    renderWithProviders(<SearchPanel embedded />);

    await waitFor(() => expect(radio(BLOCK_ALL)).toHaveAttribute('aria-checked', 'true'));
    expect(screen.queryByPlaceholderText(PLACEHOLDER)).toBeNull();
  });

  test('switching Block → Custom keeps the previously typed hosts', async () => {
    renderWithProviders(<SearchPanel embedded />);
    const textarea = (await screen.findByPlaceholderText(PLACEHOLDER)) as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: 'example.com' } });

    fireEvent.click(radio(BLOCK_ALL));
    await waitFor(() => expect(screen.queryByPlaceholderText(PLACEHOLDER)).toBeNull());
    await waitFor(() => expect(hoisted.updateSearchSettings).toHaveBeenCalled());
    await screen.findByText('settings.search.statusSaved');
    fireEvent.click(radio(CUSTOM));

    const reopened = (await screen.findByPlaceholderText(PLACEHOLDER)) as HTMLTextAreaElement;
    expect(reopened.value).toBe('example.com');
  });
});
