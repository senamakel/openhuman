import debug from 'debug';
import { type ReactNode, useCallback, useEffect, useMemo, useState } from 'react';

import {
  type ClientConfig,
  openhumanGetClientConfig,
  openhumanUpdateModelSettings,
} from '../../../utils/tauriCommands';
import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';

const log = debug('settings:backend-provider');

const KEY_PLACEHOLDER = '••••••••••••••••';

interface ProviderPreset {
  /** Stable identifier — also drives the preset card key. */
  id: string;
  label: string;
  /** OpenAI-compatible base URL ending in `/v1`. Empty = use OpenHuman default. */
  apiUrl: string;
  /** Suggested default model id; user can override after picking. */
  suggestedModel: string;
  /** Short hint shown beneath the preset row when this preset is active. */
  note: string;
  /** Tailwind classes for the logo chip background + foreground. */
  badge: { bg: string; fg: string };
  /**
   * Inline SVG glyph rendered inside the badge. Kept as simple geometric
   * marks rather than reproductions of brand logos to avoid trademark
   * issues — the colour + shape is enough for users to recognise the row.
   */
  glyph: ReactNode;
}

/**
 * Curated list of known OpenAI-compatible providers. The core uses
 * `OpenAiCompatibleProvider` (`/chat/completions` shape) so providers must
 * speak that protocol — Anthropic native (`/v1/messages`) does not, so it is
 * intentionally surfaced via OpenRouter rather than as its own preset.
 */
const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    id: 'openhuman',
    label: 'OpenHuman',
    apiUrl: '',
    suggestedModel: '',
    note: 'Hosted OpenHuman backend — uses your signed-in session, no API key required.',
    badge: { bg: 'bg-primary-500', fg: 'text-white' },
    glyph: (
      <svg viewBox="0 0 24 24" className="w-4 h-4" fill="currentColor" aria-hidden="true">
        <circle cx="12" cy="8" r="3" />
        <path d="M5 20c0-3.866 3.134-7 7-7s7 3.134 7 7v1H5v-1z" />
      </svg>
    ),
  },
  {
    id: 'openai',
    label: 'OpenAI',
    apiUrl: 'https://api.openai.com/v1',
    suggestedModel: 'gpt-4o-mini',
    note: 'Use a key from platform.openai.com. Common models: gpt-4o, gpt-4o-mini, o1-mini.',
    badge: { bg: 'bg-stone-900', fg: 'text-white' },
    glyph: (
      <svg
        viewBox="0 0 24 24"
        className="w-4 h-4"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        aria-hidden="true">
        <circle cx="12" cy="12" r="3" />
        <path
          d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M5.6 18.4l2.1-2.1M16.3 7.7l2.1-2.1"
          strokeLinecap="round"
        />
      </svg>
    ),
  },
  {
    id: 'openrouter',
    label: 'OpenRouter',
    apiUrl: 'https://openrouter.ai/api/v1',
    suggestedModel: 'anthropic/claude-3.5-sonnet',
    note: 'Routes to OpenAI, Anthropic, Google, Meta, Mistral and more under one key (openrouter.ai). Use this for Anthropic models.',
    badge: { bg: 'bg-amber-500', fg: 'text-white' },
    glyph: (
      <svg
        viewBox="0 0 24 24"
        className="w-4 h-4"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true">
        <path d="M3 12h6l3-4M3 12l3 4h6M21 8l-4 4 4 4" />
        <circle cx="3" cy="12" r="1.2" fill="currentColor" />
      </svg>
    ),
  },
  {
    id: 'ollama',
    label: 'Ollama (local)',
    apiUrl: 'http://localhost:11434/v1',
    suggestedModel: 'llama3.1',
    note: 'Local Ollama runtime via its OpenAI-compatible endpoint. API key is ignored — leave blank.',
    badge: { bg: 'bg-sage-600', fg: 'text-white' },
    glyph: (
      <svg viewBox="0 0 24 24" className="w-4 h-4" fill="currentColor" aria-hidden="true">
        <path d="M7 4c-2 0-3 2-3 4v8c0 2 1 4 3 4h2v-3a3 3 0 0 1 6 0v3h2c2 0 3-2 3-4V8c0-2-1-4-3-4h-1v2a2 2 0 0 1-4 0V4H7zm1 5a1 1 0 1 1 0 2 1 1 0 0 1 0-2zm8 0a1 1 0 1 1 0 2 1 1 0 0 1 0-2z" />
      </svg>
    ),
  },
  {
    id: 'custom',
    label: 'Custom',
    apiUrl: '',
    suggestedModel: '',
    note: 'Any other endpoint that speaks the OpenAI /chat/completions shape (vLLM, LiteLLM, LM Studio, self-hosted gateways).',
    badge: { bg: 'bg-stone-200', fg: 'text-stone-600' },
    glyph: (
      <svg
        viewBox="0 0 24 24"
        className="w-4 h-4"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true">
        <path d="M10.3 4.3a1.7 1.7 0 0 1 3.4 0 1.7 1.7 0 0 0 2.5 1 1.7 1.7 0 0 1 2.4 2.4 1.7 1.7 0 0 0 1 2.5 1.7 1.7 0 0 1 0 3.4 1.7 1.7 0 0 0-1 2.5 1.7 1.7 0 0 1-2.4 2.4 1.7 1.7 0 0 0-2.5 1 1.7 1.7 0 0 1-3.4 0 1.7 1.7 0 0 0-2.5-1 1.7 1.7 0 0 1-2.4-2.4 1.7 1.7 0 0 0-1-2.5 1.7 1.7 0 0 1 0-3.4 1.7 1.7 0 0 0 1-2.5 1.7 1.7 0 0 1 2.4-2.4 1.7 1.7 0 0 0 2.5-1z" />
        <circle cx="12" cy="12" r="2.5" />
      </svg>
    ),
  },
];

function detectPreset(apiUrl: string): ProviderPreset {
  const trimmed = apiUrl.trim();
  if (!trimmed) return PROVIDER_PRESETS[0];
  const match = PROVIDER_PRESETS.find(p => p.apiUrl && p.apiUrl === trimmed);
  if (match) return match;
  return PROVIDER_PRESETS[PROVIDER_PRESETS.length - 1]; // custom
}

/**
 * Configure a custom OpenAI-compatible inference backend (#1342).
 *
 * Leaving both fields blank falls back to the hosted OpenHuman backend.
 * Setting `api_url` + `api_key` routes inference through any OpenAI-compatible
 * provider (Ollama via `/v1`, vLLM, OpenRouter, self-hosted gateways, etc.).
 *
 * The api_key is stored on the user's machine in `config.toml`. It is sent
 * over the local Tauri↔core RPC only at write time; subsequent reads return
 * only `api_key_set: bool` so the secret never leaves the core process once
 * persisted.
 */
const BackendProviderPanel = () => {
  const { navigateBack, breadcrumbs } = useSettingsNavigation();
  const [loaded, setLoaded] = useState(false);
  const [client, setClient] = useState<ClientConfig | null>(null);
  const [apiUrl, setApiUrl] = useState('');
  const [defaultModel, setDefaultModel] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [apiKeyDirty, setApiKeyDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<{ kind: 'idle' | 'ok' | 'error'; message: string }>({
    kind: 'idle',
    message: '',
  });

  const load = useCallback(async () => {
    try {
      log('[backend-provider] loading client config');
      const response = await openhumanGetClientConfig();
      const config = response.result;
      setClient(config);
      setApiUrl(config.api_url ?? '');
      setDefaultModel(config.default_model ?? '');
      setApiKey('');
      setApiKeyDirty(false);
      setLoaded(true);
    } catch (err) {
      console.warn('[backend-provider] failed to load client config', err);
      setStatus({
        kind: 'error',
        message:
          err instanceof Error
            ? `Failed to load current settings: ${err.message}`
            : 'Failed to load current settings.',
      });
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    setStatus({ kind: 'idle', message: '' });
    try {
      await openhumanUpdateModelSettings({
        api_url: apiUrl,
        // Only send api_key when the user has actively touched the field —
        // otherwise the existing stored key (which is never echoed back to
        // the UI) stays intact across saves.
        api_key: apiKeyDirty ? apiKey : undefined,
        default_model: defaultModel,
      });
      setStatus({ kind: 'ok', message: 'Backend provider settings saved.' });
      await load();
    } catch (err) {
      console.warn('[backend-provider] save failed', err);
      setStatus({
        kind: 'error',
        message:
          err instanceof Error ? `Failed to save: ${err.message}` : 'Failed to save settings.',
      });
    } finally {
      setSaving(false);
    }
  }, [apiKey, apiKeyDirty, apiUrl, defaultModel, load]);

  const activePreset = useMemo(() => detectPreset(apiUrl), [apiUrl]);

  const applyPreset = useCallback(
    (preset: ProviderPreset) => {
      setApiUrl(preset.apiUrl);
      // Only fill the suggested model when the user hasn't typed their own.
      // Avoids clobbering a deliberate model choice when they're just trying
      // a different endpoint.
      if (preset.suggestedModel && !defaultModel.trim()) {
        setDefaultModel(preset.suggestedModel);
      }
      setStatus({ kind: 'idle', message: '' });
    },
    [defaultModel]
  );

  const handleClearKey = useCallback(async () => {
    setSaving(true);
    setStatus({ kind: 'idle', message: '' });
    try {
      await openhumanUpdateModelSettings({ api_key: '' });
      setStatus({ kind: 'ok', message: 'API key cleared.' });
      await load();
    } catch (err) {
      console.warn('[backend-provider] clear key failed', err);
      setStatus({
        kind: 'error',
        message:
          err instanceof Error ? `Failed to clear key: ${err.message}` : 'Failed to clear key.',
      });
    } finally {
      setSaving(false);
    }
  }, [load]);

  return (
    <div>
      <SettingsHeader
        title="Backend Provider"
        showBackButton={true}
        onBack={navigateBack}
        breadcrumbs={breadcrumbs}
      />
      <div className="p-4 space-y-5">
        <p className="text-sm text-stone-500 leading-relaxed">
          Route inference through any OpenAI-compatible provider — the hosted OpenHuman backend, a
          self-hosted gateway (vLLM, OpenRouter, LiteLLM…), or a local runtime exposing the OpenAI{' '}
          <code className="text-xs bg-stone-100 px-1 rounded">/v1</code> shape. Leave both fields
          blank to use the default OpenHuman backend.
        </p>

        {!loaded ? (
          <div className="text-sm text-stone-400 animate-pulse">Loading current settings…</div>
        ) : (
          <>
            <section className="space-y-2">
              <label className="block text-xs font-semibold uppercase tracking-wide text-stone-500">
                Provider
              </label>
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
                {PROVIDER_PRESETS.map(preset => {
                  const selected = preset.id === activePreset.id;
                  return (
                    <button
                      key={preset.id}
                      type="button"
                      onClick={() => applyPreset(preset)}
                      className={`flex items-center gap-2 rounded-lg border px-3 py-2 text-left transition-colors ${
                        selected
                          ? 'border-primary-400 bg-primary-50 ring-1 ring-primary-200'
                          : 'border-stone-200 bg-white hover:border-stone-300 hover:bg-stone-50'
                      }`}
                      aria-pressed={selected}>
                      <span
                        className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-full ${preset.badge.bg} ${preset.badge.fg}`}
                        aria-hidden="true">
                        {preset.glyph}
                      </span>
                      <span className="text-sm font-medium text-stone-800 truncate">
                        {preset.label}
                      </span>
                    </button>
                  );
                })}
              </div>
              <p className="text-xs text-stone-400">{activePreset.note}</p>
            </section>

            <section className="space-y-2">
              <label
                htmlFor="backend-api-url"
                className="block text-xs font-semibold uppercase tracking-wide text-stone-500">
                API URL
              </label>
              <input
                id="backend-api-url"
                type="url"
                value={apiUrl}
                onChange={e => setApiUrl(e.target.value)}
                placeholder="https://api.openai.com/v1 — or leave blank for default"
                className="w-full rounded-lg border border-stone-200 bg-white px-3 py-2 text-sm text-stone-900 placeholder:text-stone-400 focus:border-primary-400 focus:outline-none focus:ring-1 focus:ring-primary-200"
                autoComplete="off"
                spellCheck={false}
              />
              <p className="text-xs text-stone-400">
                Base URL of the OpenAI-compatible chat-completions endpoint. The path{' '}
                <code className="bg-stone-100 px-1 rounded">/chat/completions</code> is appended
                automatically.
              </p>
            </section>

            <section className="space-y-2">
              <div className="flex items-center justify-between">
                <label
                  htmlFor="backend-api-key"
                  className="block text-xs font-semibold uppercase tracking-wide text-stone-500">
                  API Key
                </label>
                {client?.api_key_set && (
                  <button
                    type="button"
                    onClick={handleClearKey}
                    disabled={saving}
                    className="text-xs text-coral-600 hover:text-coral-700 disabled:opacity-50">
                    Clear stored key
                  </button>
                )}
              </div>
              <input
                id="backend-api-key"
                type="password"
                value={apiKey}
                onChange={e => {
                  setApiKey(e.target.value);
                  setApiKeyDirty(true);
                }}
                placeholder={
                  client?.api_key_set ? `${KEY_PLACEHOLDER} (replace to change)` : 'sk-…'
                }
                className="w-full rounded-lg border border-stone-200 bg-white px-3 py-2 text-sm text-stone-900 placeholder:text-stone-400 focus:border-primary-400 focus:outline-none focus:ring-1 focus:ring-primary-200"
                autoComplete="off"
                spellCheck={false}
              />
              <p className="text-xs text-stone-400">
                Stored locally in <code className="bg-stone-100 px-1 rounded">config.toml</code> and
                never echoed back to the UI.{' '}
                {client?.api_key_set ? 'A key is currently saved.' : 'No key is currently saved.'}
              </p>
            </section>

            <section className="space-y-2">
              <label
                htmlFor="backend-default-model"
                className="block text-xs font-semibold uppercase tracking-wide text-stone-500">
                Default Model
              </label>
              <input
                id="backend-default-model"
                type="text"
                value={defaultModel}
                onChange={e => setDefaultModel(e.target.value)}
                placeholder="gpt-4o-mini, llama3.1:70b, …"
                className="w-full rounded-lg border border-stone-200 bg-white px-3 py-2 text-sm text-stone-900 placeholder:text-stone-400 focus:border-primary-400 focus:outline-none focus:ring-1 focus:ring-primary-200"
                autoComplete="off"
                spellCheck={false}
              />
              <p className="text-xs text-stone-400">
                Model identifier passed to the provider when no per-request override is set.
              </p>
            </section>

            <div className="flex items-center gap-3 pt-1">
              <button
                type="button"
                onClick={handleSave}
                disabled={saving}
                className="rounded-lg bg-primary-500 px-4 py-2 text-sm font-medium text-white hover:bg-primary-600 disabled:opacity-50">
                {saving ? 'Saving…' : 'Save'}
              </button>
              {status.kind === 'ok' && (
                <span className="text-sm text-sage-700">{status.message}</span>
              )}
              {status.kind === 'error' && (
                <span className="text-sm text-coral-700">{status.message}</span>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
};

export default BackendProviderPanel;
