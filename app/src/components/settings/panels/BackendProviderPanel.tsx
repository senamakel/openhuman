import debug from 'debug';
import { useCallback, useEffect, useState } from 'react';

import {
  type ClientConfig,
  openhumanGetClientConfig,
  openhumanUpdateModelSettings,
} from '../../../utils/tauriCommands';
import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';

const log = debug('settings:backend-provider');

const KEY_PLACEHOLDER = '••••••••••••••••';

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
