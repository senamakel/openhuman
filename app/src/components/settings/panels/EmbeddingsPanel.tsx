/**
 * Embeddings settings panel — provider selection, API keys, model + dimensions.
 *
 * Mirrors the search-engine panel pattern: radio cards for each provider,
 * inline API key entry for providers that need one, model/dimension selects
 * that constrain to the provider's catalog presets, and a destructive confirm
 * dialog when a dimension change invalidates stored vectors.
 */
import { useCallback, useEffect, useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import {
  clearEmbeddingsApiKey,
  type EmbeddingsSettings,
  type EmbeddingsTestResult,
  loadEmbeddingsSettings,
  setEmbeddingsApiKey,
  testEmbeddingsConnection,
  updateEmbeddingsSettings,
} from '../../../services/api/embeddingsApi';
import SettingsHeader from '../components/SettingsHeader';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';

type Status =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'saving' }
  | { kind: 'saved' }
  | { kind: 'error'; message: string };

const EmbeddingsPanel = () => {
  const { t } = useT();
  const { navigateBack, breadcrumbs } = useSettingsNavigation();

  const [settings, setSettings] = useState<EmbeddingsSettings | null>(null);
  const [status, setStatus] = useState<Status>({ kind: 'loading' });

  // API key input state per provider
  const [apiKeyInput, setApiKeyInput] = useState('');
  const [showKey, setShowKey] = useState(false);

  // Custom endpoint
  const [customEndpoint, setCustomEndpoint] = useState('');
  const [customModel, setCustomModel] = useState('');
  const [customDims, setCustomDims] = useState('');

  // Confirm wipe dialog
  const [pendingWipe, setPendingWipe] = useState<{
    provider?: string;
    model?: string;
    dimensions?: number;
    custom_endpoint?: string;
  } | null>(null);

  // Test connection
  const [testResult, setTestResult] = useState<EmbeddingsTestResult | null>(null);
  const [testing, setTesting] = useState(false);

  const reload = useCallback(async () => {
    try {
      const s = await loadEmbeddingsSettings();
      setSettings(s);
      setStatus({ kind: 'idle' });
    } catch (err) {
      setStatus({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  if (!settings) {
    return (
      <div className="z-10 relative">
        <SettingsHeader
          title={t('settings.embeddings.title')}
          showBackButton
          onBack={navigateBack}
          breadcrumbs={breadcrumbs}
        />
        <div className="p-4">
          <div className="rounded-lg border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 p-4 text-xs text-stone-500 dark:text-neutral-400">
            {status.kind === 'loading' ? t('common.loading') : status.kind === 'error' ? status.message : ''}
          </div>
        </div>
      </div>
    );
  }

  const selectedProvider = normalizeProvider(settings.provider);
  const currentEntry = settings.providers.find(p => p.slug === selectedProvider);
  const currentModels = currentEntry?.models ?? [];
  const currentModel = currentModels.find(m => m.id === settings.model) ?? currentModels[0];
  const allowedDims = currentModel?.allowed_dimensions ?? [];

  async function selectProvider(slug: string) {
    const entry = settings!.providers.find(p => p.slug === slug);
    const defaultModel = entry?.models[0];
    const newModel = defaultModel?.id ?? settings!.model;
    const newDims = defaultModel?.default_dimensions ?? settings!.dimensions;

    const update = {
      provider: slug,
      model: newModel,
      dimensions: newDims,
      custom_endpoint: slug === 'custom' ? customEndpoint : undefined,
    };

    await applyUpdate(update);
  }

  async function selectModel(modelId: string) {
    const model = currentModels.find(m => m.id === modelId);
    const newDims = model?.default_dimensions ?? settings!.dimensions;
    await applyUpdate({ model: modelId, dimensions: newDims });
  }

  async function selectDimensions(dims: number) {
    await applyUpdate({ dimensions: dims });
  }

  async function applyUpdate(update: {
    provider?: string;
    model?: string;
    dimensions?: number;
    custom_endpoint?: string;
  }) {
    setStatus({ kind: 'saving' });
    setTestResult(null);
    try {
      const result = await updateEmbeddingsSettings({ ...update, confirm_wipe: false });
      if (result.error === 'EMBEDDINGS_SIGNATURE_CHANGE_REQUIRES_WIPE') {
        setPendingWipe(update);
        setStatus({ kind: 'idle' });
        return;
      }
      await reload();
      setStatus({ kind: 'saved' });
    } catch (err) {
      setStatus({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }

  async function confirmWipe() {
    if (!pendingWipe) return;
    setStatus({ kind: 'saving' });
    setPendingWipe(null);
    try {
      await updateEmbeddingsSettings({ ...pendingWipe, confirm_wipe: true });
      await reload();
      setStatus({ kind: 'saved' });
    } catch (err) {
      setStatus({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }

  async function saveApiKey() {
    if (!apiKeyInput.trim() || !currentEntry) return;
    setStatus({ kind: 'saving' });
    try {
      await setEmbeddingsApiKey(selectedProvider, apiKeyInput.trim());
      setApiKeyInput('');
      setShowKey(false);
      await reload();
      setStatus({ kind: 'saved' });
    } catch (err) {
      setStatus({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }

  async function clearKey() {
    if (!currentEntry) return;
    setStatus({ kind: 'saving' });
    try {
      await clearEmbeddingsApiKey(selectedProvider);
      await reload();
      setStatus({ kind: 'saved' });
    } catch (err) {
      setStatus({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }

  async function handleTestConnection() {
    setTesting(true);
    setTestResult(null);
    try {
      const result = await testEmbeddingsConnection();
      setTestResult(result);
    } catch (err) {
      setTestResult({
        success: false,
        provider: selectedProvider,
        model: settings!.model,
        error: err instanceof Error ? err.message : String(err),
      });
    } finally {
      setTesting(false);
    }
  }

  async function applyCustom() {
    await applyUpdate({
      provider: 'custom',
      model: customModel || 'embedding',
      dimensions: Number(customDims) || 1024,
      custom_endpoint: customEndpoint,
    });
  }

  return (
    <div className="z-10 relative">
      <SettingsHeader
        title={t('settings.embeddings.title')}
        showBackButton
        onBack={navigateBack}
        breadcrumbs={breadcrumbs}
      />

      <div className="p-4 space-y-4">
        <p className="text-xs text-stone-500 dark:text-neutral-400 leading-relaxed">
          {t('settings.embeddings.description')}
        </p>

        {/* Provider selection */}
        <div
          className="bg-white dark:bg-neutral-900 rounded-xl border border-neutral-200 dark:border-neutral-800 overflow-hidden"
          role="radiogroup"
          aria-label={t('settings.embeddings.providerAria')}>
          {settings.providers.map((entry, idx) => {
            const selected = entry.slug === selectedProvider;
            return (
              <button
                key={entry.slug}
                type="button"
                role="radio"
                aria-checked={selected}
                onClick={() => void selectProvider(entry.slug)}
                className={`w-full flex items-start gap-3 px-4 py-3 text-left transition-colors focus:outline-none focus-visible:bg-primary-50 dark:focus-visible:bg-primary-900/30 ${
                  idx !== 0 ? 'border-t border-neutral-100 dark:border-neutral-800' : ''
                } ${
                  selected
                    ? 'bg-primary-50 dark:bg-primary-500/10'
                    : 'hover:bg-neutral-50 dark:hover:bg-neutral-800/60'
                }`}>
                <span className="flex-1 min-w-0">
                  <span className="flex items-center gap-2">
                    <span className="text-sm font-medium text-neutral-900 dark:text-neutral-100">
                      {entry.label}
                    </span>
                    {entry.requires_api_key && (
                      <span
                        className={`inline-flex items-center px-1.5 py-0.5 rounded text-[9px] font-semibold uppercase tracking-wider ${
                          entry.has_api_key
                            ? 'bg-sage-100 text-sage-700 dark:bg-sage-900/40 dark:text-sage-200'
                            : 'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-200'
                        }`}>
                        {entry.has_api_key
                          ? t('settings.embeddings.statusConfigured')
                          : t('settings.embeddings.statusNeedsKey')}
                      </span>
                    )}
                  </span>
                  <span className="block mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">
                    {entry.description}
                  </span>
                </span>
                {selected && (
                  <svg
                    className="w-5 h-5 text-primary-500 flex-shrink-0 mt-0.5"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                    aria-hidden>
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M5 13l4 4L19 7"
                    />
                  </svg>
                )}
              </button>
            );
          })}
        </div>

        {/* API key editor (when provider requires it) */}
        {currentEntry?.requires_api_key && (
          <div className="rounded-xl border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 p-3">
            <div className="flex items-center justify-between mb-2">
              <label className="text-xs font-semibold text-stone-700 dark:text-neutral-200">
                {t('settings.embeddings.apiKeyLabel').replace('{provider}', currentEntry.label)}
              </label>
            </div>
            <div className="flex items-center gap-2">
              <input
                type={showKey ? 'text' : 'password'}
                value={apiKeyInput}
                onChange={e => setApiKeyInput(e.target.value)}
                placeholder={
                  currentEntry.has_api_key
                    ? t('settings.embeddings.placeholderStored')
                    : t('settings.embeddings.placeholderKey')
                }
                className="flex-1 min-w-0 px-2 py-1.5 rounded-md border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs font-mono text-stone-900 dark:text-neutral-100 focus:border-primary-500 focus:outline-none focus:ring-1 focus:ring-primary-500"
              />
              <button
                type="button"
                onClick={() => setShowKey(s => !s)}
                className="px-2 py-1.5 rounded-md border border-stone-200 dark:border-neutral-800 text-xs text-stone-600 dark:text-neutral-300 hover:bg-stone-50 dark:hover:bg-neutral-800">
                {showKey ? t('settings.embeddings.hide') : t('settings.embeddings.show')}
              </button>
              <button
                type="button"
                onClick={() => void saveApiKey()}
                disabled={apiKeyInput.trim().length === 0}
                className="px-3 py-1.5 rounded-md bg-primary-500 hover:bg-primary-600 text-white text-xs font-medium disabled:opacity-50">
                {t('settings.embeddings.save')}
              </button>
              {currentEntry.has_api_key && (
                <button
                  type="button"
                  onClick={() => void clearKey()}
                  className="px-2 py-1.5 rounded-md border border-coral-200 dark:border-coral-500/30 text-xs text-coral-600 dark:text-coral-300 hover:bg-coral-50 dark:hover:bg-coral-500/10">
                  {t('settings.embeddings.clear')}
                </button>
              )}
            </div>
            <p className="mt-1.5 text-[10px] text-stone-400 dark:text-neutral-500">
              {t('settings.embeddings.keyStoredEncrypted')}
            </p>
          </div>
        )}

        {/* Custom endpoint config */}
        {selectedProvider === 'custom' && (
          <div className="rounded-xl border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 p-3 space-y-2">
            <label className="text-xs font-semibold text-stone-700 dark:text-neutral-200">
              {t('settings.embeddings.customEndpoint')}
            </label>
            <input
              type="text"
              value={customEndpoint}
              onChange={e => setCustomEndpoint(e.target.value)}
              placeholder="https://your-endpoint.com/v1"
              className="w-full px-2 py-1.5 rounded-md border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs font-mono text-stone-900 dark:text-neutral-100 focus:border-primary-500 focus:outline-none focus:ring-1 focus:ring-primary-500"
            />
            <div className="flex gap-2">
              <input
                type="text"
                value={customModel}
                onChange={e => setCustomModel(e.target.value)}
                placeholder={t('settings.embeddings.customModelPlaceholder')}
                className="flex-1 px-2 py-1.5 rounded-md border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs font-mono text-stone-900 dark:text-neutral-100 focus:border-primary-500 focus:outline-none focus:ring-1 focus:ring-primary-500"
              />
              <input
                type="number"
                value={customDims}
                onChange={e => setCustomDims(e.target.value)}
                placeholder={t('settings.embeddings.customDimsPlaceholder')}
                className="w-24 px-2 py-1.5 rounded-md border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs font-mono text-stone-900 dark:text-neutral-100 focus:border-primary-500 focus:outline-none focus:ring-1 focus:ring-primary-500"
              />
            </div>
            <button
              type="button"
              onClick={() => void applyCustom()}
              disabled={!customEndpoint.trim()}
              className="px-3 py-1.5 rounded-md bg-primary-500 hover:bg-primary-600 text-white text-xs font-medium disabled:opacity-50">
              {t('settings.embeddings.applyCustom')}
            </button>
          </div>
        )}

        {/* Model & dimensions selectors (for catalog-based providers) */}
        {currentModels.length > 0 && selectedProvider !== 'custom' && (
          <div className="rounded-xl border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 p-3 space-y-3">
            {currentModels.length > 1 && (
              <div>
                <label className="block text-xs font-semibold text-stone-700 dark:text-neutral-200 mb-1">
                  {t('settings.embeddings.model')}
                </label>
                <select
                  value={settings.model}
                  onChange={e => void selectModel(e.target.value)}
                  className="w-full px-2 py-1.5 rounded-md border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-stone-900 dark:text-neutral-100 focus:border-primary-500 focus:outline-none focus:ring-1 focus:ring-primary-500">
                  {currentModels.map(m => (
                    <option key={m.id} value={m.id}>
                      {m.label} ({m.id})
                    </option>
                  ))}
                </select>
              </div>
            )}

            {allowedDims.length > 1 && (
              <div>
                <label className="block text-xs font-semibold text-stone-700 dark:text-neutral-200 mb-1">
                  {t('settings.embeddings.dimensions')}
                </label>
                <select
                  value={settings.dimensions}
                  onChange={e => void selectDimensions(Number(e.target.value))}
                  className="w-full px-2 py-1.5 rounded-md border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-xs text-stone-900 dark:text-neutral-100 focus:border-primary-500 focus:outline-none focus:ring-1 focus:ring-primary-500">
                  {allowedDims.map(d => (
                    <option key={d} value={d}>
                      {d}
                    </option>
                  ))}
                </select>
              </div>
            )}
          </div>
        )}

        {/* Test connection */}
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => void handleTestConnection()}
            disabled={testing || selectedProvider === 'none'}
            className="px-3 py-1.5 rounded-md border border-stone-200 dark:border-neutral-800 text-xs text-stone-700 dark:text-neutral-200 hover:bg-stone-50 dark:hover:bg-neutral-800 disabled:opacity-50">
            {testing ? t('settings.embeddings.testing') : t('settings.embeddings.testConnection')}
          </button>
          {testResult && (
            <span
              className={`text-xs ${
                testResult.success
                  ? 'text-sage-600 dark:text-sage-300'
                  : 'text-coral-600 dark:text-coral-300'
              }`}>
              {testResult.success
                ? t('settings.embeddings.testSuccess').replace('{dims}', String(testResult.actual_dimensions ?? '?'))
                : t('settings.embeddings.testFailed').replace('{error}', testResult.error ?? '')}
            </span>
          )}
        </div>

        {/* Status bar */}
        <div
          role="status"
          aria-live="polite"
          className="text-xs min-h-[1rem] text-stone-500 dark:text-neutral-400">
          {status.kind === 'saving' && t('settings.embeddings.saving')}
          {status.kind === 'saved' && t('settings.embeddings.saved')}
          {status.kind === 'error' && (
            <span className="text-coral-600 dark:text-coral-300">
              {t('settings.embeddings.errorPrefix')}: {status.message}
            </span>
          )}
        </div>
      </div>

      {/* Confirm wipe dialog */}
      {pendingWipe && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
          <div className="mx-4 max-w-sm w-full rounded-2xl bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-700 p-6 shadow-xl space-y-4">
            <h3 className="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
              {t('settings.embeddings.wipeTitle')}
            </h3>
            <p className="text-xs text-neutral-600 dark:text-neutral-400 leading-relaxed">
              {t('settings.embeddings.wipeBody')}
            </p>
            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setPendingWipe(null)}
                className="px-4 py-2 rounded-lg text-xs font-medium text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800">
                {t('settings.embeddings.cancel')}
              </button>
              <button
                type="button"
                onClick={() => void confirmWipe()}
                className="px-4 py-2 rounded-lg text-xs font-medium bg-coral-500 hover:bg-coral-600 text-white">
                {t('settings.embeddings.confirmWipe')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

function normalizeProvider(raw: string): string {
  if (raw === 'cloud') return 'managed';
  if (raw.startsWith('custom:')) return 'custom';
  return raw;
}

export default EmbeddingsPanel;
