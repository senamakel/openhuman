import { fireEvent, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  loadAISettings,
  loadLocalProviderSnapshot,
  saveAISettings,
  setCloudProviderKey,
} from '../../../../services/api/aiSettingsApi';
import { renderWithProviders } from '../../../../test/test-utils';
import AIPanel from '../AIPanel';

vi.mock('../../../../services/api/aiSettingsApi', () => ({
  ALL_WORKLOADS: [
    'reasoning',
    'agentic',
    'coding',
    'memory',
    'embeddings',
    'heartbeat',
    'learning',
    'subconscious',
  ],
  loadAISettings: vi.fn(),
  saveAISettings: vi.fn(),
  loadLocalProviderSnapshot: vi.fn(),
  setCloudProviderKey: vi.fn(),
  clearCloudProviderKey: vi.fn(),
  serializeProviderRef: vi.fn((r: { kind: string; providerSlug?: string; model?: string }) =>
    r.kind === 'openhuman'
      ? 'openhuman'
      : r.kind === 'local'
        ? `ollama:${r.model}`
        : `${r.providerSlug}:${r.model}`
  ),
  localProvider: { download: vi.fn(), applyPreset: vi.fn() },
}));

vi.mock('../../hooks/useSettingsNavigation', () => ({
  useSettingsNavigation: () => ({
    navigateBack: vi.fn(),
    navigateToSettings: vi.fn(),
    breadcrumbs: [],
  }),
}));

const baseSettings = {
  cloudProviders: [
    {
      id: 'p_oh_x',
      slug: 'openhuman',
      label: 'OpenHuman',
      endpoint: 'https://api.openhuman.ai/v1',
      auth_style: 'openhuman_jwt' as const,
      has_api_key: false,
    },
  ],
  routing: {
    reasoning: { kind: 'openhuman' as const },
    agentic: { kind: 'openhuman' as const },
    coding: { kind: 'openhuman' as const },
    memory: { kind: 'openhuman' as const },
    embeddings: { kind: 'openhuman' as const },
    heartbeat: { kind: 'openhuman' as const },
    learning: { kind: 'openhuman' as const },
    subconscious: { kind: 'openhuman' as const },
  },
};

const baseLocalSnapshot = { status: null, diagnostics: null, presets: null, installedModels: [] };

describe('AIPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadAISettings).mockResolvedValue(baseSettings);
    vi.mocked(loadLocalProviderSnapshot).mockResolvedValue(baseLocalSnapshot);
  });

  it('renders the LLM Providers + Routing top-level section headers', async () => {
    renderWithProviders(<AIPanel />);
    await waitFor(() => expect(screen.getAllByText(/^LLM Providers$/).length).toBeGreaterThan(0));
    // The Local provider sub-section was removed entirely.
    expect(screen.queryByText(/Local provider/i)).not.toBeInTheDocument();
    // The old "Auth" header was renamed to "LLM Providers"; "Cloud providers"
    // sub-label is gone in favour of the chip toggles.
    expect(screen.queryByText(/^Auth$/)).not.toBeInTheDocument();
    expect(screen.queryByText(/^Cloud providers$/)).not.toBeInTheDocument();
    expect(screen.getAllByText(/^Routing$/).length).toBeGreaterThan(0);
  });

  it('renders the OpenHuman primary card after load', async () => {
    renderWithProviders(<AIPanel />);
    // The OpenHuman label now appears in multiple places (provider card,
    // each workload routing row's "↳ OpenHuman" resolution hint), so we
    // assert at-least-one match rather than getByText.
    await waitFor(() => expect(screen.getAllByText(/OpenHuman/i).length).toBeGreaterThan(0));
  });

  it('renders all eight workload labels', async () => {
    renderWithProviders(<AIPanel />);
    await waitFor(() => expect(screen.getByText('Reasoning')).toBeInTheDocument());
    for (const label of [
      'Reasoning',
      'Agentic',
      'Coding',
      'Memory summarization',
      'Embeddings',
      'Heartbeat',
      /Learning/,
      'Subconscious',
    ]) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
  });

  // ─── auth_style preservation ────────────────────────────────────────────────

  it('preserves auth_style: "anthropic" through save when Anthropic provider is configured', async () => {
    const settingsWithAnthropic = {
      cloudProviders: [
        {
          id: 'p_anthropic_1',
          slug: 'anthropic',
          label: 'Anthropic',
          endpoint: 'https://api.anthropic.com/v1',
          auth_style: 'anthropic' as const,
          has_api_key: true,
        },
      ],
      routing: {
        reasoning: {
          kind: 'cloud' as const,
          providerSlug: 'anthropic',
          model: 'claude-3-5-sonnet-20241022',
        },
        agentic: { kind: 'openhuman' as const },
        coding: { kind: 'openhuman' as const },
        memory: { kind: 'openhuman' as const },
        embeddings: { kind: 'openhuman' as const },
        heartbeat: { kind: 'openhuman' as const },
        learning: { kind: 'openhuman' as const },
        subconscious: { kind: 'openhuman' as const },
      },
    };

    vi.mocked(loadAISettings).mockResolvedValue(settingsWithAnthropic);
    vi.mocked(saveAISettings).mockResolvedValue(undefined);

    renderWithProviders(<AIPanel />);

    // Wait for load.
    await waitFor(() => expect(screen.getAllByText(/Anthropic/i).length).toBeGreaterThan(0));

    // Trigger a routing change so the SaveBar appears, then save.
    // Click the "Default" button on the Reasoning row to change routing.
    const defaultButtons = screen.getAllByText('Default');
    fireEvent.click(defaultButtons[0]);

    // SaveBar should appear.
    await waitFor(() => expect(screen.getByText(/unsaved change/i)).toBeInTheDocument());

    // Click Save in the SaveBar.
    const saveButton = screen.getByRole('button', { name: /^Save$/i });
    fireEvent.click(saveButton);

    await waitFor(() => expect(vi.mocked(saveAISettings)).toHaveBeenCalled());

    // Verify auth_style was passed through correctly in the next AISettings arg.
    const [, nextSettings] = vi.mocked(saveAISettings).mock.calls[0];
    const anthropicProvider = nextSettings.cloudProviders.find(
      (p: { slug: string }) => p.slug === 'anthropic'
    );
    expect(anthropicProvider).toBeDefined();
    expect(anthropicProvider!.auth_style).toBe('anthropic');
  });

  // ─── chip toggle: toggle ON opens API-key dialog ────────────────────────────

  it('clicking the OpenAI chip toggle (when disabled) opens the API-key dialog', async () => {
    // Load with no openai provider → chip is off.
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });

    renderWithProviders(<AIPanel />);
    await waitFor(() => expect(screen.getAllByText(/OpenAI/i).length).toBeGreaterThan(0));

    // Find the "Connect OpenAI" switch button and click it.
    const connectSwitch = screen.getByRole('switch', { name: /Connect OpenAI/i });
    fireEvent.click(connectSwitch);

    // ProviderKeyDialog should appear.
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );
    // The input for the API key should be visible.
    expect(screen.getByLabelText(/API key/i)).toBeInTheDocument();
  });

  it('clicking the Custom chip (when disabled) opens the CloudProviderEditor, not the key dialog', async () => {
    // Load with no custom provider → chip is off.
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });

    renderWithProviders(<AIPanel />);
    await waitFor(() => expect(screen.getAllByText(/Custom/i).length).toBeGreaterThan(0));

    // Find the "Connect Custom" switch and click it.
    const connectSwitch = screen.getByRole('switch', { name: /Connect Custom/i });
    fireEvent.click(connectSwitch);

    // The full CloudProviderEditor should appear (has "Add cloud provider" heading).
    await waitFor(() => expect(screen.getByText(/Add cloud provider/i)).toBeInTheDocument());
    // The simple ProviderKeyDialog should NOT appear.
    expect(screen.queryByRole('dialog', { name: /Connect Custom/i })).not.toBeInTheDocument();
  });

  // ─── chip toggle: toggle OFF scrubs routing entries ──────────────────────────

  it('toggling OFF an enabled provider scrubs routing entries that reference it', async () => {
    const settingsWithOpenAI = {
      cloudProviders: [
        {
          id: 'p_openai_1',
          slug: 'openai',
          label: 'OpenAI',
          endpoint: 'https://api.openai.com/v1',
          auth_style: 'bearer' as const,
          has_api_key: true,
        },
      ],
      routing: {
        reasoning: { kind: 'cloud' as const, providerSlug: 'openai', model: 'gpt-4o' },
        agentic: { kind: 'cloud' as const, providerSlug: 'openai', model: 'gpt-4o-mini' },
        coding: { kind: 'openhuman' as const },
        memory: { kind: 'openhuman' as const },
        embeddings: { kind: 'openhuman' as const },
        heartbeat: { kind: 'openhuman' as const },
        learning: { kind: 'openhuman' as const },
        subconscious: { kind: 'openhuman' as const },
      },
    };
    vi.mocked(loadAISettings).mockResolvedValue(settingsWithOpenAI);
    vi.mocked(saveAISettings).mockResolvedValue(undefined);

    renderWithProviders(<AIPanel />);

    // Wait for load — OpenAI chip should be ON.
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Disconnect OpenAI/i })).toBeInTheDocument()
    );

    // Toggle OFF.
    fireEvent.click(screen.getByRole('switch', { name: /Disconnect OpenAI/i }));

    // A SaveBar must appear because the draft changed.
    await waitFor(() => expect(screen.getByText(/unsaved change/i)).toBeInTheDocument());

    // Save to capture the nextSettings arg.
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    await waitFor(() => expect(vi.mocked(saveAISettings)).toHaveBeenCalled());

    const [, nextSettings] = vi.mocked(saveAISettings).mock.calls[0];

    // Provider should be gone.
    expect(
      nextSettings.cloudProviders.find((p: { slug: string }) => p.slug === 'openai')
    ).toBeUndefined();

    // Routing entries that were pinned to openai must be reset to openhuman.
    expect(nextSettings.routing.reasoning).toEqual({ kind: 'openhuman' });
    expect(nextSettings.routing.agentic).toEqual({ kind: 'openhuman' });
    // Entries that were already openhuman remain unchanged.
    expect(nextSettings.routing.coding).toEqual({ kind: 'openhuman' });
  });

  // ─── ProviderKeyDialog placeholder variants ───────────────────────────────────

  it('shows sk-ant-... placeholder for Anthropic in the API-key dialog', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect Anthropic/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect Anthropic/i }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect Anthropic/i })).toBeInTheDocument()
    );
    expect(screen.getByPlaceholderText('sk-ant-...')).toBeInTheDocument();
  });

  it('shows sk-or-... placeholder for OpenRouter in the API-key dialog', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect OpenRouter/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect OpenRouter/i }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect OpenRouter/i })).toBeInTheDocument()
    );
    expect(screen.getByPlaceholderText('sk-or-...')).toBeInTheDocument();
  });

  it('shows empty error when Save clicked with empty key in ProviderKeyDialog', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect OpenAI/i }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );
    // Click Save without entering any key.
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    await waitFor(() =>
      expect(screen.getByText(/Please paste your API key to continue/i)).toBeInTheDocument()
    );
  });

  // ─── ProviderKeyDialog: onSubmit throws surfaced error ────────────────────────

  it('surfaces error text when onSubmit rejects inside ProviderKeyDialog', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    vi.mocked(setCloudProviderKey).mockRejectedValue(new Error('network failure'));
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect OpenAI/i }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );
    fireEvent.change(screen.getByLabelText(/API key/i), { target: { value: 'sk-test' } });
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    await waitFor(() => expect(vi.mocked(setCloudProviderKey)).toHaveBeenCalled());
    // Dialog stays open; no new "Disconnect" switch (chip stays off).
    expect(screen.queryByRole('switch', { name: /Disconnect OpenAI/i })).not.toBeInTheDocument();
  });

  // ─── ProviderKeyDialog: Cancel closes dialog ────────────────────────────────

  it('Cancel button closes the API-key dialog without adding a provider', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect OpenAI/i }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('button', { name: /^Cancel$/i }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: /Connect OpenAI/i })).not.toBeInTheDocument()
    );
    expect(screen.queryByRole('switch', { name: /Disconnect OpenAI/i })).not.toBeInTheDocument();
  });

  // ─── OpenAI chip toggle ON: happy path ───────────────────────────────────────

  it('toggling ON OpenAI chip (happy path) adds provider to draft and closes dialog', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    vi.mocked(setCloudProviderKey).mockResolvedValue(undefined);
    vi.mocked(saveAISettings).mockResolvedValue(undefined);
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect OpenAI/i }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );
    fireEvent.change(screen.getByLabelText(/API key/i), { target: { value: 'sk-valid' } });
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    // After success, dialog closes and chip flips to "Disconnect OpenAI".
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: /Connect OpenAI/i })).not.toBeInTheDocument()
    );
    expect(screen.getByRole('switch', { name: /Disconnect OpenAI/i })).toBeInTheDocument();
    // SaveBar must appear because draft was mutated.
    expect(screen.getByText(/unsaved change/i)).toBeInTheDocument();
  });

  // ─── LM Studio chip toggle ON: opens key dialog (endpoint field) ─────────────

  it('clicking LM Studio chip opens API-key dialog with generic placeholder', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect LM Studio/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect LM Studio/i }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect LM Studio/i })).toBeInTheDocument()
    );
    expect(screen.getByLabelText(/API key/i)).toBeInTheDocument();
  });

  it('LM Studio toggle ON happy path adds provider with endpoint, no remote key call', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    vi.mocked(setCloudProviderKey).mockResolvedValue(undefined);
    vi.mocked(saveAISettings).mockResolvedValue(undefined);
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect LM Studio/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect LM Studio/i }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect LM Studio/i })).toBeInTheDocument()
    );
    const endpointInput = screen.getByLabelText(/API key/i);
    fireEvent.change(endpointInput, { target: { value: 'http://localhost:1234/v1' } });
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    // Dialog closes and chip flips to "Disconnect LM Studio".
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: /Connect LM Studio/i })).not.toBeInTheDocument()
    );
    expect(screen.getByRole('switch', { name: /Disconnect LM Studio/i })).toBeInTheDocument();
    // setCloudProviderKey must NOT have been called (local runtime skips key store).
    expect(vi.mocked(setCloudProviderKey)).not.toHaveBeenCalled();
  });

  it('Ollama chip toggle ON adds provider and flips chip', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    vi.mocked(setCloudProviderKey).mockResolvedValue(undefined);
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect Ollama/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect Ollama/i }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect Ollama/i })).toBeInTheDocument()
    );
    fireEvent.change(screen.getByLabelText(/API key/i), {
      target: { value: 'http://localhost:11434' },
    });
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    await waitFor(() =>
      expect(screen.queryByRole('dialog', { name: /Connect Ollama/i })).not.toBeInTheDocument()
    );
    expect(screen.getByRole('switch', { name: /Disconnect Ollama/i })).toBeInTheDocument();
    expect(vi.mocked(setCloudProviderKey)).not.toHaveBeenCalled();
  });

  it('toggling OFF LM Studio chip scrubs routing entries that reference it', async () => {
    const settingsWithLmStudio = {
      cloudProviders: [
        {
          id: 'p_lmstudio_1',
          slug: 'lmstudio',
          label: 'LM Studio',
          endpoint: 'http://localhost:1234/v1',
          auth_style: 'none' as const,
          has_api_key: false,
        },
      ],
      routing: {
        reasoning: { kind: 'cloud' as const, providerSlug: 'lmstudio', model: 'lmstudio-model' },
        agentic: { kind: 'openhuman' as const },
        coding: { kind: 'openhuman' as const },
        memory: { kind: 'openhuman' as const },
        embeddings: { kind: 'openhuman' as const },
        heartbeat: { kind: 'openhuman' as const },
        learning: { kind: 'openhuman' as const },
        subconscious: { kind: 'openhuman' as const },
      },
    };
    vi.mocked(loadAISettings).mockResolvedValue(settingsWithLmStudio);
    vi.mocked(saveAISettings).mockResolvedValue(undefined);
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Disconnect LM Studio/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Disconnect LM Studio/i }));
    await waitFor(() => expect(screen.getByText(/unsaved change/i)).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    await waitFor(() => expect(vi.mocked(saveAISettings)).toHaveBeenCalled());
    const [, nextSettings] = vi.mocked(saveAISettings).mock.calls[0];
    expect(
      nextSettings.cloudProviders.find((p: { slug: string }) => p.slug === 'lmstudio')
    ).toBeUndefined();
    expect(nextSettings.routing.reasoning).toEqual({ kind: 'openhuman' });
  });

  // ─── WorkloadRow: Default button resets routing to openhuman ─────────────────

  it('clicking "Default" button on a Custom-routed workload resets it to openhuman', async () => {
    const settingsWithOpenAI = {
      cloudProviders: [
        {
          id: 'p_openai_1',
          slug: 'openai',
          label: 'OpenAI',
          endpoint: 'https://api.openai.com/v1',
          auth_style: 'bearer' as const,
          has_api_key: true,
        },
      ],
      routing: {
        reasoning: { kind: 'cloud' as const, providerSlug: 'openai', model: 'gpt-4o' },
        agentic: { kind: 'openhuman' as const },
        coding: { kind: 'openhuman' as const },
        memory: { kind: 'openhuman' as const },
        embeddings: { kind: 'openhuman' as const },
        heartbeat: { kind: 'openhuman' as const },
        learning: { kind: 'openhuman' as const },
        subconscious: { kind: 'openhuman' as const },
      },
    };
    vi.mocked(loadAISettings).mockResolvedValue(settingsWithOpenAI);
    vi.mocked(saveAISettings).mockResolvedValue(undefined);
    renderWithProviders(<AIPanel />);
    await waitFor(() => expect(screen.getByText('Reasoning')).toBeInTheDocument());
    // Reasoning row shows "OpenAI · gpt-4o" and the Custom button is active.
    // Click "Default" on the first Default button (Reasoning row).
    const defaultButtons = screen.getAllByRole('button', { name: /^Default$/i });
    fireEvent.click(defaultButtons[0]);
    await waitFor(() => expect(screen.getByText(/unsaved change/i)).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    await waitFor(() => expect(vi.mocked(saveAISettings)).toHaveBeenCalled());
    const [, nextSettings] = vi.mocked(saveAISettings).mock.calls[0];
    expect(nextSettings.routing.reasoning).toEqual({ kind: 'openhuman' });
  });

  // ─── CustomRoutingDialog: open + submit cloud provider routing ────────────────

  it('CustomRoutingDialog: clicking Custom on a workload row opens the dialog', async () => {
    renderWithProviders(<AIPanel />);
    await waitFor(() => expect(screen.getByText('Reasoning')).toBeInTheDocument());
    // Click the "Custom" button for the Reasoning row.
    const customButtons = screen.getAllByRole('button', { name: /^Custom$/i });
    fireEvent.click(customButtons[0]);
    await waitFor(() =>
      expect(
        screen.getByRole('dialog', { name: /Custom routing for Reasoning/i })
      ).toBeInTheDocument()
    );
  });

  it('CustomRoutingDialog: shows "No custom providers" message when none are configured', async () => {
    // No cloud providers except OpenHuman (baseSettings default).
    renderWithProviders(<AIPanel />);
    await waitFor(() => expect(screen.getByText('Reasoning')).toBeInTheDocument());
    const customButtons = screen.getAllByRole('button', { name: /^Custom$/i });
    fireEvent.click(customButtons[0]);
    await waitFor(() =>
      expect(
        screen.getByRole('dialog', { name: /Custom routing for Reasoning/i })
      ).toBeInTheDocument()
    );
    expect(screen.getByText(/No custom providers are set up yet/i)).toBeInTheDocument();
  });

  it('CustomRoutingDialog: Cancel closes the dialog without changing routing', async () => {
    renderWithProviders(<AIPanel />);
    await waitFor(() => expect(screen.getByText('Reasoning')).toBeInTheDocument());
    const customButtons = screen.getAllByRole('button', { name: /^Custom$/i });
    fireEvent.click(customButtons[0]);
    await waitFor(() =>
      expect(
        screen.getByRole('dialog', { name: /Custom routing for Reasoning/i })
      ).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('button', { name: /^Cancel$/i }));
    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: /Custom routing for Reasoning/i })
      ).not.toBeInTheDocument()
    );
    // No SaveBar should appear.
    expect(screen.queryByText(/unsaved change/i)).not.toBeInTheDocument();
  });

  it('CustomRoutingDialog: submitting a cloud routing entry updates routing and triggers SaveBar', async () => {
    const settingsWithOpenAI = {
      cloudProviders: [
        {
          id: 'p_openai_1',
          slug: 'openai',
          label: 'OpenAI',
          endpoint: 'https://api.openai.com/v1',
          auth_style: 'bearer' as const,
          has_api_key: true,
        },
      ],
      routing: {
        reasoning: { kind: 'openhuman' as const },
        agentic: { kind: 'openhuman' as const },
        coding: { kind: 'openhuman' as const },
        memory: { kind: 'openhuman' as const },
        embeddings: { kind: 'openhuman' as const },
        heartbeat: { kind: 'openhuman' as const },
        learning: { kind: 'openhuman' as const },
        subconscious: { kind: 'openhuman' as const },
      },
    };
    vi.mocked(loadAISettings).mockResolvedValue(settingsWithOpenAI);
    vi.mocked(saveAISettings).mockResolvedValue(undefined);
    renderWithProviders(<AIPanel />);
    await waitFor(() => expect(screen.getByText('Reasoning')).toBeInTheDocument());
    // Open Custom dialog for Reasoning row.
    const customButtons = screen.getAllByRole('button', { name: /^Custom$/i });
    fireEvent.click(customButtons[0]);
    await waitFor(() =>
      expect(
        screen.getByRole('dialog', { name: /Custom routing for Reasoning/i })
      ).toBeInTheDocument()
    );
    // The dialog should have a model input (cloud source selected by default).
    const modelInput = screen.getByPlaceholderText(/model-id|openai model id/i);
    fireEvent.change(modelInput, { target: { value: 'gpt-4o' } });
    // Click Save in the dialog.
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: /Custom routing for Reasoning/i })
      ).not.toBeInTheDocument()
    );
    // SaveBar should appear.
    expect(screen.getByText(/unsaved change/i)).toBeInTheDocument();
    // The resolved line should show the model.
    expect(screen.getByText(/↳ OpenAI · gpt-4o/i)).toBeInTheDocument();
    // Save to verify the routing was updated.
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    await waitFor(() => expect(vi.mocked(saveAISettings)).toHaveBeenCalled());
    const [, nextSettings] = vi.mocked(saveAISettings).mock.calls[0];
    expect(nextSettings.routing.reasoning).toEqual({
      kind: 'cloud',
      providerSlug: 'openai',
      model: 'gpt-4o',
    });
  });

  it('CustomRoutingDialog: selecting local provider shows model select with installed models', async () => {
    const settingsWithOpenAI = {
      cloudProviders: [
        {
          id: 'p_openai_1',
          slug: 'openai',
          label: 'OpenAI',
          endpoint: 'https://api.openai.com/v1',
          auth_style: 'bearer' as const,
          has_api_key: true,
        },
      ],
      routing: {
        reasoning: { kind: 'openhuman' as const },
        agentic: { kind: 'openhuman' as const },
        coding: { kind: 'openhuman' as const },
        memory: { kind: 'openhuman' as const },
        embeddings: { kind: 'openhuman' as const },
        heartbeat: { kind: 'openhuman' as const },
        learning: { kind: 'openhuman' as const },
        subconscious: { kind: 'openhuman' as const },
      },
    };
    const localSnapshotWithModels = {
      status: { state: 'running' },
      diagnostics: { ollama_running: true, ollama_binary_path: '/usr/local/bin/ollama' },
      presets: null,
      installedModels: [{ name: 'llama3', size: 100000, family: 'llama' }],
    };
    vi.mocked(loadAISettings).mockResolvedValue(settingsWithOpenAI);
    vi.mocked(loadLocalProviderSnapshot).mockResolvedValue(localSnapshotWithModels);
    renderWithProviders(<AIPanel />);
    await waitFor(() => expect(screen.getByText('Reasoning')).toBeInTheDocument());
    const customButtons = screen.getAllByRole('button', { name: /^Custom$/i });
    fireEvent.click(customButtons[0]);
    await waitFor(() =>
      expect(
        screen.getByRole('dialog', { name: /Custom routing for Reasoning/i })
      ).toBeInTheDocument()
    );
    // Switch source to local in the Provider select.
    // The dialog has two selects (provider + model) — find provider by current value.
    const selects = screen.getAllByRole('combobox');
    const providerSelect = selects.find(s => (s as HTMLSelectElement).value.startsWith('cloud:'));
    expect(providerSelect).toBeDefined();
    fireEvent.change(providerSelect!, { target: { value: 'local:' } });
    // Model select (not text input) should now appear with llama3 option.
    await waitFor(() => expect(screen.getByText('llama3')).toBeInTheDocument());
    // Save with local model.
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));
    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: /Custom routing for Reasoning/i })
      ).not.toBeInTheDocument()
    );
    expect(screen.getByText(/unsaved change/i)).toBeInTheDocument();
  });

  // ─── CloudProviderEditor: full flow via Custom chip ───────────────────────────

  it('CloudProviderEditor: filling form and clicking Add provider adds it to draft', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    vi.mocked(setCloudProviderKey).mockResolvedValue(undefined);
    vi.mocked(saveAISettings).mockResolvedValue(undefined);
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect Custom/i })).toBeInTheDocument()
    );
    // Click the Custom chip to open the CloudProviderEditor.
    fireEvent.click(screen.getByRole('switch', { name: /Connect Custom/i }));
    await waitFor(() => expect(screen.getByText(/Add cloud provider/i)).toBeInTheDocument());

    // Fill in a custom endpoint (slug select stays "openai" by default since
    // no providers are present; change to custom).
    // Labels don't have `for` attrs — use getByRole('combobox') for the slug select.
    const slugSelect = screen.getByRole('combobox');
    fireEvent.change(slugSelect, { target: { value: 'custom' } });

    const labelInput = screen.getByPlaceholderText(/My Provider/i);
    fireEvent.change(labelInput, { target: { value: 'DeepSeek' } });

    const endpointInput = screen.getByPlaceholderText(/https:\/\/api\.example\.com\/v1/i);
    fireEvent.change(endpointInput, { target: { value: 'https://api.deepseek.com/v1' } });

    const apiKeyInput = screen.getByPlaceholderText(/sk-\.\.\./i);
    fireEvent.change(apiKeyInput, { target: { value: 'ds-test-key' } });

    fireEvent.click(screen.getByRole('button', { name: /Add provider/i }));

    await waitFor(() => expect(screen.queryByText(/Add cloud provider/i)).not.toBeInTheDocument());
    // SaveBar must appear.
    expect(screen.getByText(/unsaved change/i)).toBeInTheDocument();
    // setCloudProviderKey should have been called with the slug and key.
    expect(vi.mocked(setCloudProviderKey)).toHaveBeenCalledWith('custom', 'ds-test-key');
  });

  it('CloudProviderEditor: Cancel closes without adding a provider', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect Custom/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect Custom/i }));
    await waitFor(() => expect(screen.getByText(/Add cloud provider/i)).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: /^Cancel$/i }));
    await waitFor(() => expect(screen.queryByText(/Add cloud provider/i)).not.toBeInTheDocument());
    expect(screen.queryByText(/unsaved change/i)).not.toBeInTheDocument();
  });

  it('CloudProviderEditor: slug change updates label and endpoint', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect Custom/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect Custom/i }));
    await waitFor(() => expect(screen.getByText(/Add cloud provider/i)).toBeInTheDocument());
    // Labels don't have `for` attrs — use getByRole('combobox') for the slug select.
    const slugSelect = screen.getByRole('combobox');
    // Change to anthropic.
    fireEvent.change(slugSelect, { target: { value: 'anthropic' } });
    // Label should update to Anthropic.
    await waitFor(() => {
      const labelInput = screen.getByPlaceholderText(/My Provider/i) as HTMLInputElement;
      expect(labelInput.value).toBe('Anthropic');
    });
  });

  it('CloudProviderEditor: setCloudProviderKey failure prevents provider from being added', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    vi.mocked(setCloudProviderKey).mockRejectedValue(new Error('credential store unavailable'));
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect Custom/i })).toBeInTheDocument()
    );
    fireEvent.click(screen.getByRole('switch', { name: /Connect Custom/i }));
    await waitFor(() => expect(screen.getByText(/Add cloud provider/i)).toBeInTheDocument());

    // Fill required fields.
    const endpointInput = screen.getByPlaceholderText(/https:\/\/api\.example\.com\/v1/i);
    fireEvent.change(endpointInput, { target: { value: 'https://api.openai.com/v1' } });
    const apiKeyInput = screen.getByPlaceholderText(/sk-\.\.\./i);
    fireEvent.change(apiKeyInput, { target: { value: 'sk-fail' } });

    fireEvent.click(screen.getByRole('button', { name: /Add provider/i }));

    // setCloudProviderKey must have been called.
    await waitFor(() => expect(vi.mocked(setCloudProviderKey)).toHaveBeenCalled());
    // The editor stays open (onSubmit returned early).
    expect(screen.getByText(/Add cloud provider/i)).toBeInTheDocument();
    // No SaveBar.
    expect(screen.queryByText(/unsaved change/i)).not.toBeInTheDocument();
  });

  // ─── SaveBar: Discard resets draft ─────────────────────────────────────────

  it('Discard button in SaveBar resets draft to saved state', async () => {
    const settingsWithOpenAI = {
      cloudProviders: [
        {
          id: 'p_openai_1',
          slug: 'openai',
          label: 'OpenAI',
          endpoint: 'https://api.openai.com/v1',
          auth_style: 'bearer' as const,
          has_api_key: true,
        },
      ],
      routing: {
        reasoning: { kind: 'cloud' as const, providerSlug: 'openai', model: 'gpt-4o' },
        agentic: { kind: 'openhuman' as const },
        coding: { kind: 'openhuman' as const },
        memory: { kind: 'openhuman' as const },
        embeddings: { kind: 'openhuman' as const },
        heartbeat: { kind: 'openhuman' as const },
        learning: { kind: 'openhuman' as const },
        subconscious: { kind: 'openhuman' as const },
      },
    };
    vi.mocked(loadAISettings).mockResolvedValue(settingsWithOpenAI);
    renderWithProviders(<AIPanel />);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Disconnect OpenAI/i })).toBeInTheDocument()
    );
    // Toggle OFF to make draft dirty.
    fireEvent.click(screen.getByRole('switch', { name: /Disconnect OpenAI/i }));
    await waitFor(() => expect(screen.getByText(/unsaved change/i)).toBeInTheDocument());
    // Discard.
    fireEvent.click(screen.getByRole('button', { name: /Discard/i }));
    await waitFor(() => expect(screen.queryByText(/unsaved change/i)).not.toBeInTheDocument());
    // Chip should be back to ON.
    expect(screen.getByRole('switch', { name: /Disconnect OpenAI/i })).toBeInTheDocument();
  });

  // ─── API-key dialog: failed setCloudProviderKey does not add provider ────────

  it('when setCloudProviderKey throws, the provider is NOT added to the draft', async () => {
    vi.mocked(loadAISettings).mockResolvedValue({ ...baseSettings, cloudProviders: [] });
    // Make setCloudProviderKey reject.
    vi.mocked(setCloudProviderKey).mockRejectedValue(new Error('key store failed'));

    renderWithProviders(<AIPanel />);

    // Wait for OpenAI chip to render (disabled).
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );

    // Count provider chips before dialog interaction.
    const chipsBefore = screen.getAllByRole('switch').length;

    // Open the dialog.
    fireEvent.click(screen.getByRole('switch', { name: /Connect OpenAI/i }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: /Connect OpenAI/i })).toBeInTheDocument()
    );

    // Fill in a key and submit.
    fireEvent.change(screen.getByLabelText(/API key/i), { target: { value: 'sk-bad-key' } });
    fireEvent.click(screen.getByRole('button', { name: /^Save$/i }));

    // The panel silently catches the setCloudProviderKey error and does NOT
    // mutate the draft. Because the panel's onSubmit returns (doesn't throw),
    // the dialog's handleSave resolves without entering its catch block, leaving
    // the dialog in the 'saving' phase with the button showing "Saving…".
    //
    // Wait for setCloudProviderKey to have been called (confirms the flow ran).
    await waitFor(() => expect(vi.mocked(setCloudProviderKey)).toHaveBeenCalled());

    // The dialog must still be open (setKeyDialogFor was never set to null).
    expect(screen.getByRole('dialog', { name: /Connect OpenAI/i })).toBeInTheDocument();

    // The number of provider toggle switches must not have grown — the failed
    // provider was never added to the draft.
    expect(screen.getAllByRole('switch').length).toBe(chipsBefore);

    // Specifically: no "Disconnect OpenAI" switch (chip is still in off state).
    expect(screen.queryByRole('switch', { name: /Disconnect OpenAI/i })).not.toBeInTheDocument();
  });
});
