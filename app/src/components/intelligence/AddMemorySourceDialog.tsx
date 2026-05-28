/**
 * Dialog for adding a new memory source.
 *
 * Shows a kind picker, then a form with kind-specific fields.
 * On submit, calls `addMemorySource` and notifies the parent.
 */
import { useCallback, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import {
  addMemorySource,
  type MemorySourceEntry,
  SOURCE_KIND_ICONS,
  SOURCE_KIND_LABELS,
  type SourceKind,
} from '../../services/memorySourcesService';

interface AddMemorySourceDialogProps {
  open: boolean;
  onClose: () => void;
  onAdded: (source: MemorySourceEntry) => void;
}

const NON_COMPOSIO_KINDS: SourceKind[] = [
  'folder',
  'github_repo',
  'rss_feed',
  'web_page',
  'twitter_query',
];

export function AddMemorySourceDialog({ open, onClose, onAdded }: AddMemorySourceDialogProps) {
  const { t } = useT();
  const [kind, setKind] = useState<SourceKind | null>(null);
  const [label, setLabel] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Kind-specific fields
  const [path, setPath] = useState('');
  const [glob, setGlob] = useState('**/*.md');
  const [url, setUrl] = useState('');
  const [branch, setBranch] = useState('main');
  const [query, setQuery] = useState('');
  const [selector, setSelector] = useState('');

  const reset = useCallback(() => {
    setKind(null);
    setLabel('');
    setPath('');
    setGlob('**/*.md');
    setUrl('');
    setBranch('main');
    setQuery('');
    setSelector('');
    setError(null);
  }, []);

  const handleClose = useCallback(() => {
    reset();
    onClose();
  }, [onClose, reset]);

  const handleSubmit = useCallback(async () => {
    if (!kind || !label.trim()) return;
    setSubmitting(true);
    setError(null);

    try {
      const params: Record<string, unknown> = { kind, label: label.trim(), enabled: true };

      switch (kind) {
        case 'folder':
          params.path = path.trim();
          params.glob = glob.trim() || '**/*.md';
          break;
        case 'github_repo':
          params.url = url.trim();
          params.branch = branch.trim() || 'main';
          break;
        case 'rss_feed':
          params.url = url.trim();
          break;
        case 'web_page':
          params.url = url.trim();
          if (selector.trim()) params.selector = selector.trim();
          break;
        case 'twitter_query':
          params.query = query.trim();
          break;
      }

      const source = await addMemorySource(params as Omit<MemorySourceEntry, 'id'>);
      onAdded(source);
      handleClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  }, [kind, label, path, glob, url, branch, query, selector, onAdded, handleClose]);

  if (!open) return null;

  const isValid = kind && label.trim() && isKindFieldsValid(kind, { path, url, query });

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm">
      <div className="w-full max-w-lg rounded-xl border border-stone-200 bg-white p-6 shadow-xl dark:border-neutral-700 dark:bg-neutral-900">
        <h2 className="text-lg font-semibold text-stone-900 dark:text-neutral-100">
          {t('memorySources.addSource')}
        </h2>

        {!kind ? (
          <>
            <p className="mt-2 text-sm text-stone-500 dark:text-neutral-400">
              {t('memorySources.pickKind')}
            </p>
            <div className="mt-4 grid grid-cols-2 gap-3">
              {NON_COMPOSIO_KINDS.map(k => (
                <button
                  key={k}
                  type="button"
                  onClick={() => setKind(k)}
                  className="flex items-center gap-3 rounded-lg border border-stone-200 p-3
                             text-left transition-colors hover:border-primary-400 hover:bg-primary-50
                             dark:border-neutral-700 dark:hover:border-primary-500 dark:hover:bg-primary-500/10">
                  <span className="text-xl">{SOURCE_KIND_ICONS[k]}</span>
                  <span className="text-sm font-medium text-stone-800 dark:text-neutral-200">
                    {SOURCE_KIND_LABELS[k]}
                  </span>
                </button>
              ))}
            </div>
            <div className="mt-4 flex justify-end">
              <button
                type="button"
                onClick={handleClose}
                className="rounded-md px-4 py-2 text-sm text-stone-600 hover:text-stone-900
                           dark:text-neutral-400 dark:hover:text-neutral-100">
                {t('common.cancel')}
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="mt-1 text-sm text-stone-500 dark:text-neutral-400">
              {SOURCE_KIND_ICONS[kind]} {SOURCE_KIND_LABELS[kind]}
            </p>

            <div className="mt-4 space-y-3">
              <Field
                label={t('memorySources.label')}
                value={label}
                onChange={setLabel}
                placeholder={t('memorySources.labelPlaceholder')}
              />
              <KindFields
                kind={kind}
                path={path}
                setPath={setPath}
                glob={glob}
                setGlob={setGlob}
                url={url}
                setUrl={setUrl}
                branch={branch}
                setBranch={setBranch}
                query={query}
                setQuery={setQuery}
                selector={selector}
                setSelector={setSelector}
              />
            </div>

            {error && (
              <p className="mt-3 rounded-md bg-coral-50 p-2 text-xs text-coral-800 dark:bg-coral-500/10 dark:text-coral-300">
                {error}
              </p>
            )}

            <div className="mt-5 flex items-center justify-between">
              <button
                type="button"
                onClick={() => {
                  setKind(null);
                  setError(null);
                }}
                className="text-sm text-stone-500 hover:text-stone-800 dark:text-neutral-400 dark:hover:text-neutral-200">
                ← {t('memorySources.backToKinds')}
              </button>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={handleClose}
                  className="rounded-md px-4 py-2 text-sm text-stone-600 hover:text-stone-900
                             dark:text-neutral-400 dark:hover:text-neutral-100">
                  {t('common.cancel')}
                </button>
                <button
                  type="button"
                  onClick={handleSubmit}
                  disabled={!isValid || submitting}
                  className="rounded-md bg-primary-500 px-4 py-2 text-sm font-semibold text-white
                             shadow-sm transition-colors hover:bg-primary-600
                             disabled:cursor-not-allowed disabled:opacity-50">
                  {submitting ? t('memorySources.adding') : t('memorySources.add')}
                </button>
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function isKindFieldsValid(
  kind: SourceKind,
  fields: { path: string; url: string; query: string }
): boolean {
  switch (kind) {
    case 'folder':
      return fields.path.trim().length > 0;
    case 'github_repo':
    case 'rss_feed':
    case 'web_page':
      return fields.url.trim().length > 0;
    case 'twitter_query':
      return fields.query.trim().length > 0;
    default:
      return true;
  }
}

interface FieldProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
}

function Field({ label, value, onChange, placeholder, type = 'text' }: FieldProps) {
  return (
    <label className="block">
      <span className="text-xs font-medium text-stone-600 dark:text-neutral-400">{label}</span>
      <input
        type={type}
        value={value}
        onChange={e => onChange(e.target.value)}
        placeholder={placeholder}
        className="mt-1 block w-full rounded-md border border-stone-300 bg-white px-3 py-2
                   text-sm text-stone-900 placeholder-stone-400
                   focus:border-primary-400 focus:outline-none focus:ring-1 focus:ring-primary-400
                   dark:border-neutral-600 dark:bg-neutral-800 dark:text-neutral-100
                   dark:placeholder-neutral-500 dark:focus:border-primary-500"
      />
    </label>
  );
}

interface KindFieldsProps {
  kind: SourceKind;
  path: string;
  setPath: (v: string) => void;
  glob: string;
  setGlob: (v: string) => void;
  url: string;
  setUrl: (v: string) => void;
  branch: string;
  setBranch: (v: string) => void;
  query: string;
  setQuery: (v: string) => void;
  selector: string;
  setSelector: (v: string) => void;
}

function KindFields(props: KindFieldsProps) {
  const { t } = useT();
  switch (props.kind) {
    case 'folder':
      return (
        <>
          <Field
            label={t('memorySources.folderPath')}
            value={props.path}
            onChange={props.setPath}
            placeholder="/Users/you/notes"
          />
          <Field
            label={t('memorySources.globPattern')}
            value={props.glob}
            onChange={props.setGlob}
            placeholder="**/*.md"
          />
        </>
      );
    case 'github_repo':
      return (
        <>
          <Field
            label={t('memorySources.repoUrl')}
            value={props.url}
            onChange={props.setUrl}
            placeholder="https://github.com/org/repo"
          />
          <Field
            label={t('memorySources.branch')}
            value={props.branch}
            onChange={props.setBranch}
            placeholder="main"
          />
        </>
      );
    case 'rss_feed':
      return (
        <Field
          label={t('memorySources.feedUrl')}
          value={props.url}
          onChange={props.setUrl}
          placeholder="https://example.com/feed.xml"
        />
      );
    case 'web_page':
      return (
        <>
          <Field
            label={t('memorySources.pageUrl')}
            value={props.url}
            onChange={props.setUrl}
            placeholder="https://example.com/article"
          />
          <Field
            label={t('memorySources.cssSelector')}
            value={props.selector}
            onChange={props.setSelector}
            placeholder="article"
          />
        </>
      );
    case 'twitter_query':
      return (
        <Field
          label={t('memorySources.searchQuery')}
          value={props.query}
          onChange={props.setQuery}
          placeholder="from:user AI safety"
        />
      );
    default:
      return null;
  }
}
