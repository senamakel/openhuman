import { useEffect, useMemo, useState } from 'react';

import { useT } from '../../../../lib/i18n/I18nContext';
import { listProviderModels, type ModelInfo } from '../../../../services/api/aiSettingsApi';
import Button from '../../../ui/Button';
import { ModalShell } from '../../../ui/ModalShell';
import TextField from '../../../ui/TextField';
import {
  CLAUDE_CODE_DEFAULT_MODEL,
  type CloudProvider,
  type CustomDialogSource,
  type OllamaModel,
  slugTone,
} from './aiPanelTypes';
import { ModelEntryField, useModelEntryMode } from './ModelEntryField';
import { ProviderSwatch } from './ProviderListRow';

type TFn = (key: string, fallback?: string) => string;

export interface ProviderModelSelection {
  source: CustomDialogSource;
  model: string;
  /** Provider-reported context window for this exact model, when known. */
  contextWindow?: number | null;
}

interface ProviderModelPickerDialogProps {
  /**
   * Offer "Managed by OpenHuman" as the first source. Default `true`: managed
   * is the product's own routing and must stay reachable from anywhere a model
   * is chosen, or picking a specific model becomes a one-way door.
   *
   * Pass `false` only where managed would contradict the surface itself — the
   * "Use Your Own Models" card being the one case: choosing managed inside it
   * would silently flip the routing mode out from under the card.
   */
  allowManaged?: boolean;
  cloudProviders: CloudProvider[];
  localModels: OllamaModel[];
  ollamaRunning: boolean;
  claudeCodeEnabled: boolean;
  initial: ProviderModelSelection | null;
  onClose: () => void;
  onSelect: (selection: ProviderModelSelection) => void;
}

const sourceKey = (source: CustomDialogSource) =>
  source.kind === 'cloud' ? `cloud:${source.providerSlug}` : source.kind;

/** Managed needs no model id — the product chooses one per workload. */
const isManaged = (source: CustomDialogSource | null): boolean => source?.kind === 'managed';

const sourceLabel = (source: CustomDialogSource, providers: CloudProvider[], t: TFn) =>
  source.kind === 'managed'
    ? t('settings.ai.managedSourceLabel')
    : source.kind === 'cloud'
      ? (providers.find(provider => provider.slug === source.providerSlug)?.label ??
        source.providerSlug)
      : source.kind === 'local'
        ? 'Ollama'
        : 'Claude Code';

const sourceSlug = (source: CustomDialogSource) =>
  source.kind === 'managed'
    ? 'openhuman'
    : source.kind === 'cloud'
      ? source.providerSlug
      : source.kind === 'local'
        ? 'ollama'
        : 'claude-code';

const sourceDetail = (source: CustomDialogSource, t: TFn) =>
  source.kind === 'managed'
    ? t('settings.ai.managedSourceDetail')
    : source.kind === 'cloud'
      ? 'Cloud provider'
      : source.kind === 'local'
        ? 'Local runtime'
        : 'CLI provider';

/**
 * Shared, searchable provider and model chooser. It owns discovery and
 * selection only; callers keep their own persistence, validation, and test
 * flows, so the same dialog can serve global and per-workload routing.
 */
export function ProviderModelPickerDialog({
  allowManaged = true,
  cloudProviders,
  localModels,
  ollamaRunning,
  claudeCodeEnabled,
  initial,
  onClose,
  onSelect,
}: ProviderModelPickerDialogProps) {
  const { t } = useT();
  const sources = useMemo<CustomDialogSource[]>(
    () => [
      // First, and before any configured provider: it is the default the app
      // ships with, and the one a user needs to find again after trying their
      // own key.
      ...(allowManaged ? ([{ kind: 'managed' as const }] as const) : []),
      ...cloudProviders.map(provider => ({ kind: 'cloud' as const, providerSlug: provider.slug })),
      ...(ollamaRunning && localModels.length > 0 ? ([{ kind: 'local' as const }] as const) : []),
      ...(claudeCodeEnabled ? ([{ kind: 'claude-code' as const }] as const) : []),
    ],
    [allowManaged, claudeCodeEnabled, cloudProviders, localModels.length, ollamaRunning]
  );
  const [query, setQuery] = useState('');
  const [source, setSource] = useState<CustomDialogSource | null>(
    initial?.source ?? sources[0] ?? null
  );
  const [model, setModel] = useState(initial?.model ?? '');
  const [catalog, setCatalog] = useState<ModelInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [catalogRequest, setCatalogRequest] = useState(0);

  const selectedCloudProvider =
    source?.kind === 'cloud'
      ? cloudProviders.find(provider => provider.slug === source.providerSlug)
      : undefined;
  const modelEntryMode = useModelEntryMode({
    endpoint: selectedCloudProvider?.endpoint,
    model,
    catalogIds: catalog.map(candidate => candidate.id),
  });

  useEffect(() => {
    if (!source || source.kind !== 'cloud') {
      setCatalog(source?.kind === 'local' ? localModels : []);
      return;
    }
    let active = true;
    setLoading(true);
    setCatalog([]);
    setCatalogError(null);
    void listProviderModels(source.providerSlug)
      .then(models => {
        if (!active) return;
        setCatalog(models);
        setLoading(false);
      })
      .catch(() => {
        if (active) {
          setLoading(false);
          setCatalogError('Could not load models from this provider.');
        }
      });
    return () => {
      active = false;
    };
  }, [catalogRequest, localModels, source]);

  const filteredSources = sources.filter(candidate =>
    sourceLabel(candidate, cloudProviders, t)
      .toLocaleLowerCase()
      .includes(query.toLocaleLowerCase())
  );
  const selectSource = (nextSource: CustomDialogSource) => {
    setSource(nextSource);
    setModel(nextSource.kind === 'claude-code' ? CLAUDE_CODE_DEFAULT_MODEL : '');
    modelEntryMode.syncToEndpoint(
      nextSource.kind === 'cloud'
        ? cloudProviders.find(provider => provider.slug === nextSource.providerSlug)?.endpoint
        : undefined
    );
  };

  return (
    <ModalShell
      title="Choose provider and model"
      titleId="provider-model-picker-title"
      subtitle="Search configured providers and available models."
      onClose={onClose}
      maxWidthClassName="max-w-3xl"
      contentClassName="p-0"
      footer={
        <div className="flex justify-end gap-2">
          <Button type="button" variant="secondary" size="sm" onClick={onClose}>
            Cancel
          </Button>
          <Button
            type="button"
            variant="primary"
            size="sm"
            // Managed carries no model id, so requiring one would leave the
            // only always-available option permanently unselectable.
            disabled={!source || (!isManaged(source) && !model.trim())}
            onClick={() => {
              if (!source) return;
              if (isManaged(source)) {
                onSelect({ source, model: '', contextWindow: null });
                return;
              }
              const selectedModel = catalog.find(candidate => candidate.id === model.trim());
              onSelect({
                source,
                model: model.trim(),
                contextWindow:
                  selectedModel && (selectedModel.context_window ?? 0) > 0
                    ? selectedModel.context_window
                    : null,
              });
            }}>
            Use this model
          </Button>
        </div>
      }>
      <div className="border-b border-line-subtle p-4">
        <TextField
          value={query}
          onChange={event => setQuery(event.target.value)}
          placeholder="Search providers and models"
          aria-label="Search providers and models"
          autoFocus
        />
      </div>
      <div className="grid min-h-80 grid-cols-1 divide-y divide-line-subtle md:grid-cols-[13rem_1fr] md:divide-x md:divide-y-0">
        <div className="p-2">
          <p className="px-2 pb-2 text-xs font-medium text-content-muted">Providers</p>
          <div className="space-y-1">
            {filteredSources.map(candidate => {
              const selected = source && sourceKey(candidate) === sourceKey(source);
              return (
                <Button
                  key={sourceKey(candidate)}
                  type="button"
                  variant="tertiary"
                  size="sm"
                  onClick={() => selectSource(candidate)}
                  className={`h-auto w-full justify-start gap-3 px-2.5 py-2 ${selected ? 'bg-surface-muted' : ''}`}>
                  <ProviderSwatch
                    slug={sourceSlug(candidate)}
                    label={sourceLabel(candidate, cloudProviders, t)}
                    tone={slugTone(sourceSlug(candidate))}
                  />
                  <span className="flex min-w-0 flex-col items-start gap-0.5">
                    <span className="truncate text-sm font-medium">
                      {sourceLabel(candidate, cloudProviders, t)}
                    </span>
                    <span className="text-xs font-normal text-content-muted">
                      {sourceDetail(candidate, t)}
                    </span>
                  </span>
                </Button>
              );
            })}
          </div>
        </div>
        <div className="min-w-0 p-4">
          {isManaged(source) ? (
            // No model field: managed picks per workload and keeps that choice
            // current, so there is nothing here for the user to fill in.
            <div data-testid="model-picker-managed-pane" className="space-y-2">
              <p className="text-sm font-medium text-content">
                {t('settings.ai.managedSourceLabel')}
              </p>
              <p className="text-xs leading-relaxed text-content-muted">
                {t('settings.ai.routing.managedDesc')}
              </p>
            </div>
          ) : source?.kind === 'cloud' ? (
            <ModelEntryField
              mode={modelEntryMode}
              model={model}
              onModelChange={setModel}
              catalog={catalog}
              catalogLoading={loading}
              catalogError={catalogError}
              onRetry={() => setCatalogRequest(request => request + 1)}
              label="Model"
              placeholder="Enter a model ID"
              analyticsId="settings-ai-model-picker-manual-entry"
            />
          ) : (
            <>
              <p className="mb-2 text-xs font-medium text-content-muted">Model</p>
              <TextField
                value={model}
                onChange={event => setModel(event.target.value)}
                placeholder="Enter a model ID"
                aria-label="Model"
                mono
              />
            </>
          )}
          {source && !isManaged(source) && source.kind !== 'cloud' ? (
            <div className="mt-3 max-h-56 space-y-1 overflow-y-auto">
              {source?.kind === 'claude-code' ? (
                <p className="text-sm text-content-muted">
                  Use a Claude Code model alias or model ID.
                </p>
              ) : (
                catalog.map(candidate => (
                  <Button
                    key={candidate.id}
                    type="button"
                    variant="tertiary"
                    size="sm"
                    onClick={() => setModel(candidate.id)}
                    className={`w-full justify-start font-mono ${model === candidate.id ? 'bg-surface-muted' : ''}`}>
                    {candidate.id}
                  </Button>
                ))
              )}
            </div>
          ) : null}
        </div>
      </div>
    </ModalShell>
  );
}

export default ProviderModelPickerDialog;
