/**
 * Registry-based memory sources panel.
 *
 * Shows all user-configured memory sources from the memory_sources
 * registry (folders, GitHub repos, RSS feeds, web pages, Twitter
 * queries) with controls to add, toggle, and remove sources.
 *
 * Composio-backed sources (gmail, slack, etc.) continue to be shown
 * by the existing MemorySources component. This component handles
 * the non-composio source kinds plus any composio sources that were
 * manually added to the registry.
 */
import { useCallback, useEffect, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import {
  listMemorySources,
  type MemorySourceEntry,
  removeMemorySource,
  SOURCE_KIND_ICONS,
  SOURCE_KIND_LABELS,
  updateMemorySource,
} from '../../services/memorySourcesService';
import type { ToastNotification } from '../../types/intelligence';
import { AddMemorySourceDialog } from './AddMemorySourceDialog';

interface MemorySourcesRegistryProps {
  onToast?: (toast: Omit<ToastNotification, 'id'>) => void;
}

export function MemorySourcesRegistry({ onToast }: MemorySourcesRegistryProps) {
  const { t } = useT();
  const [sources, setSources] = useState<MemorySourceEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [dialogOpen, setDialogOpen] = useState(false);

  const loadSources = useCallback(async () => {
    try {
      const list = await listMemorySources();
      setSources(list.filter(s => s.kind !== 'composio'));
    } catch (err) {
      console.warn('[ui-flow][memory-sources-registry] load failed', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadSources();
  }, [loadSources]);

  const handleToggle = useCallback(
    async (source: MemorySourceEntry) => {
      try {
        const updated = await updateMemorySource(source.id, { enabled: !source.enabled });
        setSources(prev => prev.map(s => (s.id === updated.id ? updated : s)));
      } catch (err) {
        onToast?.({
          type: 'error',
          title: t('memorySources.toggleFailed'),
          message: err instanceof Error ? err.message : String(err),
        });
      }
    },
    [onToast, t]
  );

  const handleRemove = useCallback(
    async (source: MemorySourceEntry) => {
      try {
        await removeMemorySource(source.id);
        setSources(prev => prev.filter(s => s.id !== source.id));
        onToast?.({ type: 'success', title: t('memorySources.removed'), message: source.label });
      } catch (err) {
        onToast?.({
          type: 'error',
          title: t('memorySources.removeFailed'),
          message: err instanceof Error ? err.message : String(err),
        });
      }
    },
    [onToast, t]
  );

  const handleAdded = useCallback(
    (source: MemorySourceEntry) => {
      setSources(prev => [...prev, source]);
      onToast?.({ type: 'success', title: t('memorySources.added'), message: source.label });
    },
    [onToast, t]
  );

  return (
    <section
      className="rounded-lg border border-stone-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900"
      data-testid="memory-sources-registry">
      <header className="mb-3 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-stone-700 dark:text-neutral-200">
          {t('memorySources.customSources')}
        </h3>
        <button
          type="button"
          onClick={() => setDialogOpen(true)}
          className="inline-flex items-center gap-1 rounded-md bg-primary-500 px-3 py-1.5
                     text-xs font-semibold text-white shadow-sm transition-colors
                     hover:bg-primary-600 focus:outline-none focus:ring-2 focus:ring-primary-200">
          <PlusIcon />
          {t('memorySources.addSource')}
        </button>
      </header>

      {loading ? (
        <p className="text-xs text-stone-500 dark:text-neutral-400">{t('common.loading')}</p>
      ) : sources.length === 0 ? (
        <p className="text-xs text-stone-500 dark:text-neutral-400">
          {t('memorySources.noCustomSources')}
        </p>
      ) : (
        <ul className="divide-y divide-stone-100 dark:divide-neutral-800">
          {sources.map(source => (
            <SourceRow
              key={source.id}
              source={source}
              onToggle={handleToggle}
              onRemove={handleRemove}
            />
          ))}
        </ul>
      )}

      <AddMemorySourceDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onAdded={handleAdded}
      />
    </section>
  );
}

interface SourceRowProps {
  source: MemorySourceEntry;
  onToggle: (source: MemorySourceEntry) => void;
  onRemove: (source: MemorySourceEntry) => void;
}

function SourceRow({ source, onToggle, onRemove }: SourceRowProps) {
  const { t } = useT();
  const icon = SOURCE_KIND_ICONS[source.kind] ?? '📄';
  const kindLabel = SOURCE_KIND_LABELS[source.kind] ?? source.kind;
  const detail = sourceDetail(source);

  return (
    <li className="flex items-center justify-between gap-3 py-3">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-base">{icon}</span>
          <span
            className={`truncate text-sm font-medium ${
              source.enabled
                ? 'text-stone-900 dark:text-neutral-100'
                : 'text-stone-400 line-through dark:text-neutral-500'
            }`}>
            {source.label}
          </span>
          <span className="rounded-md bg-stone-100 px-1.5 py-0.5 text-[10px] font-medium text-stone-500 dark:bg-neutral-800 dark:text-neutral-400">
            {kindLabel}
          </span>
        </div>
        {detail && (
          <p className="mt-0.5 truncate pl-7 text-xs text-stone-400 dark:text-neutral-500">
            {detail}
          </p>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <button
          type="button"
          onClick={() => onToggle(source)}
          title={source.enabled ? t('memorySources.disable') : t('memorySources.enable')}
          className={`relative h-5 w-9 rounded-full transition-colors ${
            source.enabled ? 'bg-primary-500' : 'bg-stone-300 dark:bg-neutral-600'
          }`}>
          <span
            className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${
              source.enabled ? 'left-[18px]' : 'left-0.5'
            }`}
          />
        </button>
        <button
          type="button"
          onClick={() => onRemove(source)}
          title={t('memorySources.remove')}
          className="rounded p-1 text-stone-400 transition-colors hover:bg-coral-50
                     hover:text-coral-600 dark:text-neutral-500 dark:hover:bg-coral-500/10
                     dark:hover:text-coral-400">
          <TrashIcon />
        </button>
      </div>
    </li>
  );
}

function sourceDetail(source: MemorySourceEntry): string | null {
  switch (source.kind) {
    case 'folder':
      return source.path ?? null;
    case 'github_repo':
      return source.url ?? null;
    case 'rss_feed':
      return source.url ?? null;
    case 'web_page':
      return source.url ?? null;
    case 'twitter_query':
      return source.query ?? null;
    default:
      return null;
  }
}

function PlusIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true">
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true">
      <path d="M3 6h18M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6" />
    </svg>
  );
}
