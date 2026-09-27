import { Save } from 'lucide-react';
import { useId, useState } from 'react';

import type {
  SearchProviderInfo,
  SearchProviderStatus,
  SearchProviderUpdate,
  SearchRoute,
} from '../../../utils/tauriCommands/config';
import Badge, { type BadgeVariant } from '../../ui/Badge';
import { InputGroupButton, InputGroupInput, InputGroupRoot } from '../../ui/InputGroup';
import Switch from '../../ui/Switch';
import { ToggleGroupItem, ToggleGroupRoot } from '../../ui/ToggleGroup';
import KeyEditor from './SearchPanelKeyEditor';

type Translate = (key: string) => string;

const STATUS_VARIANT: Record<SearchProviderStatus, BadgeVariant> = {
  ready: 'success',
  needs_key: 'warning',
  sign_in_required: 'warning',
  disabled: 'neutral',
  search_off: 'neutral',
};

/** Badge text for a provider status. Literal keys so the i18n scanner sees them. */
export function statusLabel(status: SearchProviderStatus, t: Translate): string {
  switch (status) {
    case 'ready':
      return t('settings.search.statusReady');
    case 'needs_key':
      return t('settings.search.statusNeedsKey');
    case 'sign_in_required':
      return t('settings.search.statusSignInRequired');
    default:
      return t('settings.search.statusOff');
  }
}

const withProvider = (text: string, provider: string) => text.replace('{provider}', provider);

/** Segmented-control look shared with the allowed-websites mode picker. */
export const SEGMENTED_ROOT_CLASS =
  'gap-0 overflow-hidden rounded-lg border border-line *:rounded-none *:border-0';
export const SEGMENTED_ITEM_CLASS =
  'h-auto px-2.5 py-1 text-xs font-medium data-[state=on]:bg-primary-500 data-[state=on]:text-content-inverted';

interface Props {
  provider: SearchProviderInfo;
  saving: boolean;
  /** Persist a patch for this provider; resolves true when the core accepted it. */
  onUpdate: (patch: SearchProviderUpdate) => Promise<boolean>;
  t: Translate;
}

/**
 * One search provider, rendered as a row of the Providers card: enable switch, status badge, route choice (when the
 * provider supports more than one), its key editor and any extra field the
 * core reports for it (SearXNG's instance URL). Everything shown is driven by
 * the provider entry the core returned; nothing here knows a provider by id.
 */
const SearchPanelProviderCard = ({ provider, saving, onUpdate, t }: Props) => {
  const switchId = useId();
  const [draftKey, setDraftKey] = useState('');
  const [showKey, setShowKey] = useState(false);
  const [draftBaseUrl, setDraftBaseUrl] = useState(provider.base_url ?? '');
  const testId = `search-provider-${provider.id}`;

  // A key is needed for the direct route. A provider that reports
  // `deep_research_available` (Gemini) also takes an optional key on the
  // managed route, because the key is what unlocks deep research.
  const deepResearchCapable = provider.deep_research_available !== undefined;
  const showKeyEditor = provider.takes_key && (provider.route === 'direct' || deepResearchCapable);
  const hasBaseUrl = provider.base_url !== undefined;

  const saveKey = async (value: string) => {
    if (await onUpdate({ api_key: value })) setDraftKey('');
  };

  return (
    <div data-testid={testId} data-status={provider.status} className="space-y-3 px-4 py-3">
      <div className="flex items-center gap-3">
        <label htmlFor={switchId} className="flex min-w-0 flex-1 items-center gap-2">
          <span className="truncate text-sm font-medium text-content">{provider.label}</span>
          <Badge variant={STATUS_VARIANT[provider.status]} data-testid={`${testId}-status`}>
            {statusLabel(provider.status, t)}
          </Badge>
        </label>
        <Switch
          id={switchId}
          data-testid={`${testId}-toggle`}
          aria-label={withProvider(t('settings.search.providerToggleAria'), provider.label)}
          checked={provider.enabled}
          disabled={saving}
          onCheckedChange={next => void onUpdate({ enabled: next })}
        />
      </div>

      {provider.routes.length > 1 && (
        <ToggleGroupRoot
          type="single"
          aria-label={withProvider(t('settings.search.routeAria'), provider.label)}
          value={provider.route}
          onValueChange={value => {
            if (value && value !== provider.route) void onUpdate({ route: value as SearchRoute });
          }}
          variant="secondary"
          size="xs"
          disabled={saving}
          className={SEGMENTED_ROOT_CLASS}>
          {provider.routes.map(route => (
            <ToggleGroupItem
              key={route}
              value={route}
              data-testid={`${testId}-route-${route}`}
              disabled={route === 'managed' && !provider.managed_available}
              className={SEGMENTED_ITEM_CLASS}>
              {route === 'managed'
                ? t('settings.search.routeManaged')
                : t('settings.search.routeDirect')}
            </ToggleGroupItem>
          ))}
        </ToggleGroupRoot>
      )}

      {showKeyEditor && (
        <KeyEditor
          label={withProvider(t('settings.search.apiKeyLabel'), provider.label)}
          placeholder={
            provider.key_configured
              ? t('settings.search.placeholderStored')
              : withProvider(t('settings.search.placeholderKey'), provider.label)
          }
          show={showKey}
          onToggleShow={() => setShowKey(s => !s)}
          value={draftKey}
          onChange={setDraftKey}
          onSave={() => void saveKey(draftKey)}
          onClear={() => void saveKey('')}
          configured={provider.key_configured}
          docUrl={provider.docs_url}
          disabled={saving}
          testId={`${testId}-key`}
          t={t}
        />
      )}

      {deepResearchCapable && (
        <p
          data-testid={`${testId}-deep-research`}
          className="text-xs leading-relaxed text-content-muted">
          {provider.deep_research_available
            ? withProvider(t('settings.search.deepResearchAvailable'), provider.label)
            : withProvider(t('settings.search.deepResearchHint'), provider.label)}
        </p>
      )}

      {hasBaseUrl && (
        <div className="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
          <label htmlFor={`${switchId}-base-url`} className="text-sm text-content">
            {t('settings.search.baseUrlLabel')}
          </label>
          <InputGroupRoot size="sm" className="w-full md:w-96">
            <InputGroupInput
              id={`${switchId}-base-url`}
              data-testid={`${testId}-base-url`}
              mono
              value={draftBaseUrl}
              onChange={e => setDraftBaseUrl(e.target.value)}
              spellCheck={false}
            />
            <InputGroupButton
              type="button"
              variant="primary"
              leadingIcon={<Save className="h-3.5 w-3.5" aria-hidden />}
              disabled={
                saving ||
                draftBaseUrl.trim().length === 0 ||
                draftBaseUrl.trim() === (provider.base_url ?? '')
              }
              onClick={() => void onUpdate({ base_url: draftBaseUrl.trim() })}>
              {t('settings.search.baseUrlSave')}
            </InputGroupButton>
          </InputGroupRoot>
        </div>
      )}
    </div>
  );
};

export default SearchPanelProviderCard;
