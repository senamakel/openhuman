/**
 * Composio-aware dropdown fields for the node-config forms. Instead of making
 * users hand-type a toolkit slug, an action slug, or a trigger slug, these pick
 * from the user's ACTIVE connections and the live Composio catalog:
 *
 *  - {@link ComposioToolkitField} — the connected apps the user actually has
 *    (derived from the `FlowConnection[]` the canvas already loaded), so a
 *    `trigger` app_event picks its app from a dropdown.
 *  - {@link ComposioActionField} — the real action slugs for a toolkit
 *    (`composio_list_tools`), for a `tool_call` node's action.
 *  - {@link ComposioTriggerField} — the available trigger slugs for a toolkit
 *    (`composio_list_available_triggers`), for an app_event's trigger.
 *
 * The action/trigger lists are fetched on demand (per toolkit) and cached in
 * local state; they keep the current value selectable even if the catalog fetch
 * fails, and offer a "Custom…" escape hatch so an advanced/unavailable slug is
 * never a dead end. All fetches are guarded — outside Tauri (or on error) the
 * field degrades to the custom input rather than throwing.
 */
import { useEffect, useMemo, useState } from 'react';

import { listAvailableTriggers, listTools } from '../../../../lib/composio/composioApi';
import { useT } from '../../../../lib/i18n/I18nContext';
import type { FlowConnection } from '../../../../services/api/flowsApi';
import { Field, INPUT_CLASS, MONO_CLASS } from './nodeConfigFields';

/** Sentinel select value that reveals a raw text input. */
const CUSTOM = '__custom__';

/** Prettify a toolkit slug for display (`googlesheets` → `Googlesheets`). */
function toolkitLabel(slug: string): string {
  return slug ? slug.charAt(0).toUpperCase() + slug.slice(1) : slug;
}

/** Distinct connected Composio toolkits from the canvas's loaded connections. */
export function connectedToolkits(connections: FlowConnection[]): string[] {
  const seen = new Set<string>();
  for (const c of connections) {
    if (c.kind === 'composio' && c.toolkit) seen.add(c.toolkit);
  }
  return [...seen].sort();
}

// ── toolkit (app) picker ─────────────────────────────────────────────────────

export interface ComposioToolkitFieldProps {
  label: string;
  hint?: string;
  value: string;
  onChange: (value: string) => void;
  connections: FlowConnection[];
  testId?: string;
}

export function ComposioToolkitField({
  label,
  hint,
  value,
  onChange,
  connections,
  testId,
}: ComposioToolkitFieldProps) {
  const { t } = useT();
  const toolkits = useMemo(() => connectedToolkits(connections), [connections]);

  if (toolkits.length === 0) {
    return (
      <Field label={label} hint={hint}>
        <p
          className="rounded-lg border border-dashed border-line-strong px-2.5 py-1.5 text-xs text-content-faint"
          data-testid={testId ? `${testId}-empty` : undefined}>
          {t('flows.nodeConfig.composio.noConnections')}
        </p>
      </Field>
    );
  }

  // Keep a saved-but-now-disconnected toolkit selectable so editing an existing
  // flow never silently drops it.
  const options = toolkits.includes(value) || value === '' ? toolkits : [value, ...toolkits];

  return (
    <Field label={label} hint={hint}>
      <select
        className={INPUT_CLASS}
        value={value}
        data-testid={testId}
        onChange={e => onChange(e.target.value)}>
        <option value="">{t('flows.nodeConfig.composio.selectApp')}</option>
        {options.map(tk => (
          <option key={tk} value={tk}>
            {toolkitLabel(tk)}
          </option>
        ))}
      </select>
    </Field>
  );
}

// ── shared catalog-slug dropdown (actions + triggers) ────────────────────────

interface CatalogSlugFieldProps {
  label: string;
  hint?: string;
  value: string;
  onChange: (value: string) => void;
  /** The toolkit whose catalog to load; empty → prompt to pick a connection. */
  toolkit: string;
  /** Fetch the slugs for a toolkit. */
  fetchSlugs: (toolkit: string) => Promise<string[]>;
  emptyPrompt: string;
  testId?: string;
}

function CatalogSlugField({
  label,
  hint,
  value,
  onChange,
  toolkit,
  fetchSlugs,
  emptyPrompt,
  testId,
}: CatalogSlugFieldProps) {
  const { t } = useT();
  const [slugs, setSlugs] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);
  // Custom mode is entered explicitly, or forced when the catalog can't load so
  // the field never traps the user with an empty dropdown.
  const [custom, setCustom] = useState(false);

  useEffect(() => {
    if (!toolkit) {
      setSlugs([]);
      setFailed(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setFailed(false);
    void (async () => {
      try {
        const result = await fetchSlugs(toolkit);
        if (!cancelled) setSlugs(result);
      } catch {
        if (!cancelled) setFailed(true);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [toolkit, fetchSlugs]);

  if (!toolkit) {
    return (
      <Field label={label} hint={hint}>
        <p
          className="rounded-lg border border-dashed border-line-strong px-2.5 py-1.5 text-xs text-content-faint"
          data-testid={testId ? `${testId}-needs-connection` : undefined}>
          {emptyPrompt}
        </p>
      </Field>
    );
  }

  const showCustomInput = custom || (failed && slugs.length === 0);
  // Keep the current value selectable even if it's not in the fetched list.
  const options = value && !slugs.includes(value) ? [value, ...slugs] : slugs;

  return (
    <Field label={label} hint={hint}>
      {showCustomInput ? (
        <input
          type="text"
          className={`${INPUT_CLASS} ${MONO_CLASS}`}
          value={value}
          placeholder={t('flows.nodeConfig.composio.customPlaceholder')}
          data-testid={testId ? `${testId}-custom` : undefined}
          onChange={e => onChange(e.target.value)}
        />
      ) : (
        <select
          className={INPUT_CLASS}
          value={value}
          disabled={loading}
          data-testid={testId}
          onChange={e => {
            if (e.target.value === CUSTOM) {
              setCustom(true);
              return;
            }
            onChange(e.target.value);
          }}>
          <option value="">
            {loading
              ? t('flows.nodeConfig.composio.loading')
              : t('flows.nodeConfig.composio.select')}
          </option>
          {options.map(slug => (
            <option key={slug} value={slug}>
              {slug}
            </option>
          ))}
          <option value={CUSTOM}>{t('flows.nodeConfig.composio.custom')}</option>
        </select>
      )}
    </Field>
  );
}

// ── tool_call action ─────────────────────────────────────────────────────────

async function fetchActionSlugs(toolkit: string): Promise<string[]> {
  const res = await listTools([toolkit]);
  return res.tools
    .map(tool => tool.function?.name)
    .filter((name): name is string => typeof name === 'string' && name.length > 0);
}

export function ComposioActionField(props: {
  label: string;
  hint?: string;
  value: string;
  onChange: (value: string) => void;
  toolkit: string;
  testId?: string;
}) {
  const { t } = useT();
  return (
    <CatalogSlugField
      {...props}
      fetchSlugs={fetchActionSlugs}
      emptyPrompt={t('flows.nodeConfig.tool.pickConnection')}
    />
  );
}

// ── app_event trigger ────────────────────────────────────────────────────────

async function fetchTriggerSlugs(toolkit: string): Promise<string[]> {
  const res = await listAvailableTriggers(toolkit);
  return res.triggers
    .map(trigger => trigger.slug)
    .filter((slug): slug is string => typeof slug === 'string' && slug.length > 0);
}

export function ComposioTriggerField(props: {
  label: string;
  hint?: string;
  value: string;
  onChange: (value: string) => void;
  toolkit: string;
  testId?: string;
}) {
  const { t } = useT();
  return (
    <CatalogSlugField
      {...props}
      fetchSlugs={fetchTriggerSlugs}
      emptyPrompt={t('flows.nodeConfig.trigger.pickApp')}
    />
  );
}
