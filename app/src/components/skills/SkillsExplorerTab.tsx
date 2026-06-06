import { useCallback, useEffect, useMemo, useState } from 'react';
import debug from 'debug';

import { useT } from '../../lib/i18n/I18nContext';
import {
  skillRegistryApi,
  type CatalogEntry,
} from '../../services/api/skillRegistryApi';
import {
  skillsApi,
  type InstallSkillFromUrlResult,
  type SkillSummary,
} from '../../services/api/skillsApi';
import EmptyStateCard from '../EmptyStateCard';
import InstallSkillDialog from './InstallSkillDialog';
import UninstallSkillConfirmDialog from './UninstallSkillConfirmDialog';

const log = debug('skills:explorer-tab');

function SkillFormatBadge({ format }: { format: string }) {
  const lower = format.toLowerCase();
  const label =
    lower === 'hermes'
      ? 'Hermes'
      : lower === 'legacy'
        ? 'Legacy'
        : lower === 'openclaw'
          ? 'OpenClaw'
          : 'OpenHuman';
  const colors =
    lower === 'hermes'
      ? 'bg-violet-50 text-violet-700 border-violet-200 dark:bg-violet-500/10 dark:text-violet-300 dark:border-violet-500/30'
      : lower === 'openclaw'
        ? 'bg-teal-50 text-teal-700 border-teal-200 dark:bg-teal-500/10 dark:text-teal-300 dark:border-teal-500/30'
        : lower === 'legacy'
          ? 'bg-amber-50 text-amber-700 border-amber-200 dark:bg-amber-500/10 dark:text-amber-300 dark:border-amber-500/30'
          : 'bg-primary-50 text-primary-700 border-primary-200 dark:bg-primary-500/10 dark:text-primary-300 dark:border-primary-500/30';
  return (
    <span
      className={`inline-flex items-center rounded-full border px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider ${colors}`}>
      {label}
    </span>
  );
}

function SkillScopeBadge({ scope }: { scope: string }) {
  const { t } = useT();
  const label =
    scope === 'user'
      ? t('skills.explorer.scopeUser')
      : scope === 'project'
        ? t('skills.explorer.scopeProject')
        : t('skills.explorer.scopeLegacy');
  return (
    <span className="inline-flex items-center rounded-full border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-1.5 py-0.5 text-[9px] font-medium text-stone-500 dark:text-neutral-400">
      {label}
    </span>
  );
}

function SourceBadge({ sourceId }: { sourceId: string }) {
  return (
    <span className="inline-flex items-center rounded-full border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-1.5 py-0.5 text-[9px] font-medium text-stone-500 dark:text-neutral-400">
      {sourceId}
    </span>
  );
}

interface SkillTileProps {
  skill: SkillSummary;
  onUninstall: () => void;
}

function SkillTile({ skill, onUninstall }: SkillTileProps) {
  const { t } = useT();
  const canUninstall = skill.scope === 'user';

  return (
    <div
      data-testid={`skill-explorer-tile-${skill.id}`}
      className="group flex flex-col justify-between rounded-2xl border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 p-3 transition-colors hover:bg-stone-50 dark:hover:bg-neutral-800/60">
      <div className="min-w-0">
        <div className="flex items-start justify-between gap-2">
          <div className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-xl bg-stone-100 dark:bg-neutral-800">
            <svg
              className="h-5 w-5 text-stone-500 dark:text-neutral-400"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={1.5}>
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M9.813 15.904 9 18.75l-.813-2.846a4.5 4.5 0 0 0-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 0 0 3.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 0 0 3.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 0 0-3.09 3.09ZM18.259 8.715 18 9.75l-.259-1.035a3.375 3.375 0 0 0-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 0 0 2.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 0 0 2.455 2.456L21.75 6l-1.036.259a3.375 3.375 0 0 0-2.455 2.456ZM16.894 20.567 16.5 21.75l-.394-1.183a2.25 2.25 0 0 0-1.423-1.423L13.5 18.75l1.183-.394a2.25 2.25 0 0 0 1.423-1.423l.394-1.183.394 1.183a2.25 2.25 0 0 0 1.423 1.423l1.183.394-1.183.394a2.25 2.25 0 0 0-1.423 1.423Z"
              />
            </svg>
          </div>
          <div className="flex items-center gap-1">
            <SkillFormatBadge format={skill.sourceFormat} />
            <SkillScopeBadge scope={skill.scope} />
          </div>
        </div>

        <h3 className="mt-2 line-clamp-1 text-sm font-semibold text-stone-900 dark:text-neutral-100">
          {skill.name}
        </h3>
        <p className="mt-0.5 line-clamp-2 text-[11px] leading-relaxed text-stone-500 dark:text-neutral-400">
          {skill.description || t('skills.explorer.noDescription')}
        </p>

        {skill.tags.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1">
            {skill.tags.slice(0, 3).map(tag => (
              <span
                key={tag}
                className="rounded-full bg-stone-100 dark:bg-neutral-800 px-1.5 py-0.5 text-[9px] font-medium text-stone-500 dark:text-neutral-400">
                {tag}
              </span>
            ))}
            {skill.tags.length > 3 && (
              <span className="rounded-full bg-stone-100 dark:bg-neutral-800 px-1.5 py-0.5 text-[9px] font-medium text-stone-400 dark:text-neutral-500">
                +{skill.tags.length - 3}
              </span>
            )}
          </div>
        )}
      </div>

      <div className="mt-3 flex items-center justify-between gap-2">
        {skill.version && (
          <span className="text-[10px] font-mono text-stone-400 dark:text-neutral-500">
            v{skill.version}
          </span>
        )}
        {!skill.version && <span />}
        {canUninstall && (
          <button
            type="button"
            data-testid={`skill-uninstall-${skill.id}`}
            onClick={e => {
              e.stopPropagation();
              onUninstall();
            }}
            className="rounded-lg border border-coral-200 dark:border-coral-500/30 bg-coral-50 dark:bg-coral-500/10 px-2 py-1 text-[10px] font-medium text-coral-700 dark:text-coral-300 opacity-0 transition-all group-hover:opacity-100 hover:bg-coral-100 dark:hover:bg-coral-500/20">
            {t('skills.disconnect')}
          </button>
        )}
      </div>

      {skill.warnings.length > 0 && (
        <div className="mt-2 rounded-lg border border-amber-200 dark:border-amber-500/30 bg-amber-50 dark:bg-amber-500/10 px-2 py-1.5">
          <p className="text-[10px] font-medium text-amber-700 dark:text-amber-300">
            {skill.warnings[0]}
          </p>
        </div>
      )}
    </div>
  );
}

interface CatalogTileProps {
  entry: CatalogEntry;
  installed: boolean;
  installing: boolean;
  onInstall: () => void;
}

function CatalogTile({ entry, installed, installing, onInstall }: CatalogTileProps) {
  const { t } = useT();
  return (
    <div
      data-testid={`registry-tile-${entry.id}`}
      className={`group flex flex-col justify-between rounded-2xl border p-3 transition-colors ${
        installed
          ? 'border-sage-300 bg-sage-50/60 dark:border-sage-500/30 dark:bg-sage-500/10'
          : 'border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 hover:bg-stone-50 dark:hover:bg-neutral-800/60'
      }`}>
      <div className="min-w-0">
        <div className="flex items-start justify-between gap-2">
          <div className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-xl bg-stone-100 dark:bg-neutral-800">
            <svg
              className="h-5 w-5 text-primary-500"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={1.5}>
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M12 21v-8.25M15.75 21v-8.25M8.25 21v-8.25M3 9l9-6 9 6m-1.5 12V10.332A48.36 48.36 0 0 0 12 9.75c-2.551 0-5.056.2-7.5.582V21M3 21h18M12 6.75h.008v.008H12V6.75Z"
              />
            </svg>
          </div>
          <div className="flex items-center gap-1">
            <SkillFormatBadge format={entry.format} />
            <SourceBadge sourceId={entry.source_id} />
          </div>
        </div>

        <h3 className="mt-2 line-clamp-1 text-sm font-semibold text-stone-900 dark:text-neutral-100">
          {entry.name}
        </h3>
        <p className="mt-0.5 line-clamp-2 text-[11px] leading-relaxed text-stone-500 dark:text-neutral-400">
          {entry.description}
        </p>

        {entry.tags.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1">
            {entry.tags.slice(0, 3).map(tag => (
              <span
                key={tag}
                className="rounded-full bg-stone-100 dark:bg-neutral-800 px-1.5 py-0.5 text-[9px] font-medium text-stone-500 dark:text-neutral-400">
                {tag}
              </span>
            ))}
          </div>
        )}
      </div>

      <div className="mt-3 flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          {entry.version && (
            <span className="text-[10px] font-mono text-stone-400 dark:text-neutral-500">
              v{entry.version}
            </span>
          )}
          {entry.author && (
            <span className="text-[10px] text-stone-400 dark:text-neutral-500">
              {entry.author}
            </span>
          )}
          {entry.stars != null && entry.stars > 0 && (
            <span className="text-[10px] text-stone-400 dark:text-neutral-500">
              {entry.stars}
            </span>
          )}
        </div>
        {installed ? (
          <span className="rounded-lg border border-sage-200 dark:border-sage-500/30 bg-sage-50 dark:bg-sage-500/10 px-2 py-1 text-[10px] font-medium text-sage-700 dark:text-sage-300">
            {t('skills.explorer.installed')}
          </span>
        ) : (
          <button
            type="button"
            data-testid={`registry-install-${entry.id}`}
            disabled={installing}
            onClick={e => {
              e.stopPropagation();
              onInstall();
            }}
            className="rounded-lg border border-primary-200 dark:border-primary-500/30 bg-primary-50 dark:bg-primary-500/10 px-2 py-1 text-[10px] font-medium text-primary-700 dark:text-primary-300 transition-colors hover:bg-primary-100 dark:hover:bg-primary-500/20 disabled:opacity-50">
            {installing ? t('skills.explorer.installing') : t('skills.explorer.install')}
          </button>
        )}
      </div>
    </div>
  );
}

type ExplorerView = 'installed' | 'registry';

interface SkillsExplorerTabProps {
  onToast?: (toast: { type: 'success' | 'error'; title: string; message?: string }) => void;
}

export default function SkillsExplorerTab({ onToast }: SkillsExplorerTabProps) {
  const { t } = useT();
  const [view, setView] = useState<ExplorerView>('registry');

  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [skillsLoading, setSkillsLoading] = useState(true);
  const [skillsError, setSkillsError] = useState<string | null>(null);

  const [catalog, setCatalog] = useState<CatalogEntry[]>([]);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [installingId, setInstallingId] = useState<string | null>(null);

  const [searchQuery, setSearchQuery] = useState('');
  const [formatFilter, setFormatFilter] = useState<string>('all');
  const [installDialogOpen, setInstallDialogOpen] = useState(false);
  const [uninstallTarget, setUninstallTarget] = useState<SkillSummary | null>(null);

  const fetchSkills = useCallback(async () => {
    log('fetchSkills: start');
    setSkillsLoading(true);
    setSkillsError(null);
    try {
      const result = await skillsApi.listSkills();
      log('fetchSkills: count=%d', result.length);
      setSkills(result);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log('fetchSkills: error=%s', msg);
      setSkillsError(msg);
    } finally {
      setSkillsLoading(false);
    }
  }, []);

  const fetchCatalog = useCallback(async (forceRefresh = false) => {
    log('fetchCatalog: forceRefresh=%s', forceRefresh);
    setCatalogLoading(true);
    setCatalogError(null);
    try {
      const entries = await skillRegistryApi.browse(forceRefresh);
      log('fetchCatalog: count=%d', entries.length);
      setCatalog(entries);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log('fetchCatalog: error=%s', msg);
      setCatalogError(msg);
    } finally {
      setCatalogLoading(false);
    }
  }, []);

  useEffect(() => {
    void fetchSkills();
  }, [fetchSkills]);

  useEffect(() => {
    if (view === 'registry' && catalog.length === 0 && !catalogLoading) {
      void fetchCatalog();
    }
  }, [view, catalog.length, catalogLoading, fetchCatalog]);

  const installedIds = useMemo(() => new Set(skills.map(s => s.id)), [skills]);

  const filteredSkills = useMemo(() => {
    const q = searchQuery.toLowerCase().trim();
    if (!q) return skills;
    return skills.filter(
      s =>
        s.name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q) ||
        s.tags.some(tag => tag.toLowerCase().includes(q)) ||
        s.sourceFormat.toLowerCase().includes(q)
    );
  }, [skills, searchQuery]);

  const sortedSkills = useMemo(() => {
    return [...filteredSkills].sort((a, b) => {
      if (a.sourceFormat === 'hermes' && b.sourceFormat !== 'hermes') return -1;
      if (a.sourceFormat !== 'hermes' && b.sourceFormat === 'hermes') return 1;
      return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
    });
  }, [filteredSkills]);

  const filteredCatalog = useMemo(() => {
    const q = searchQuery.toLowerCase().trim();
    return catalog.filter(entry => {
      if (formatFilter !== 'all' && entry.format !== formatFilter) return false;
      if (!q) return true;
      return (
        entry.name.toLowerCase().includes(q) ||
        entry.description.toLowerCase().includes(q) ||
        entry.tags.some(tag => tag.toLowerCase().includes(q)) ||
        entry.format.toLowerCase().includes(q) ||
        (entry.author ?? '').toLowerCase().includes(q)
      );
    });
  }, [catalog, searchQuery, formatFilter]);

  const catalogFormats = useMemo(() => {
    const formats = new Set(catalog.map(e => e.format));
    return ['all', ...Array.from(formats).sort()];
  }, [catalog]);

  const handleInstalled = useCallback(
    (result: InstallSkillFromUrlResult) => {
      log('handleInstalled: newSkills=%d', result.newSkills.length);
      void fetchSkills();
      if (result.newSkills.length > 0) {
        onToast?.({
          type: 'success',
          title: t('skills.install.installComplete'),
          message: t('skills.install.successDiscovered').replace(
            '{count}',
            String(result.newSkills.length)
          ),
        });
      }
    },
    [fetchSkills, onToast, t]
  );

  const handleUninstalled = useCallback(() => {
    log('handleUninstalled');
    void fetchSkills();
    onToast?.({
      type: 'success',
      title: t('skills.explorer.uninstallSuccess'),
    });
  }, [fetchSkills, onToast, t]);

  const handleRegistryInstall = useCallback(
    async (entry: CatalogEntry) => {
      log('handleRegistryInstall: id=%s source=%s', entry.id, entry.source_id);
      setInstallingId(entry.id);
      try {
        const result = await skillRegistryApi.install(entry.id, entry.source_id);
        void fetchSkills();
        onToast?.({
          type: 'success',
          title: t('skills.install.installComplete'),
          message: `Installed ${entry.name}${result.new_skills.length > 0 ? ` (${result.new_skills.join(', ')})` : ''}`,
        });
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        log('handleRegistryInstall: error=%s', msg);
        onToast?.({
          type: 'error',
          title: t('skills.install.errors.genericTitle'),
          message: msg,
        });
      } finally {
        setInstallingId(null);
      }
    },
    [fetchSkills, onToast, t]
  );

  const loading = view === 'installed' ? skillsLoading : catalogLoading;
  const error = view === 'installed' ? skillsError : catalogError;

  return (
    <div className="rounded-2xl border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 p-3 shadow-soft animate-fade-up">
      <div className="px-1 pb-3 pt-1">
        <div className="flex items-center justify-between gap-2">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold text-stone-900 dark:text-neutral-100">
              {t('skills.explorer.title')}
            </h2>
            <p className="mt-0.5 text-[11px] leading-relaxed text-stone-500 dark:text-neutral-400">
              {t('skills.explorer.subtitle')}
            </p>
          </div>
          <button
            type="button"
            data-testid="skill-install-from-url-btn"
            onClick={() => setInstallDialogOpen(true)}
            className="flex-shrink-0 rounded-lg border border-stone-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 px-3 py-1.5 text-xs font-medium text-stone-700 dark:text-neutral-200 shadow-sm transition-colors hover:bg-stone-50 dark:hover:bg-neutral-800 focus:outline-none focus:ring-2 focus:ring-primary-500 focus:ring-offset-1">
            {t('skills.explorer.installFromUrl')}
          </button>
        </div>
      </div>

      {/* View toggle */}
      <div className="flex gap-2 px-1 pb-3">
        <button
          type="button"
          onClick={() => setView('registry')}
          className={`rounded-full border px-3 py-1 text-xs font-medium transition-colors ${
            view === 'registry'
              ? 'border-primary-200 dark:border-primary-500/40 bg-primary-50 dark:bg-primary-500/15 text-primary-700 dark:text-primary-300'
              : 'border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-stone-600 dark:text-neutral-300 hover:bg-stone-50 dark:hover:bg-neutral-800/60'
          }`}>
          {t('skills.explorer.registryTab')}
          {catalog.length > 0 && (
            <span className="ml-1.5 text-[10px] opacity-70">{catalog.length}</span>
          )}
        </button>
        <button
          type="button"
          onClick={() => setView('installed')}
          className={`rounded-full border px-3 py-1 text-xs font-medium transition-colors ${
            view === 'installed'
              ? 'border-primary-200 dark:border-primary-500/40 bg-primary-50 dark:bg-primary-500/15 text-primary-700 dark:text-primary-300'
              : 'border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-stone-600 dark:text-neutral-300 hover:bg-stone-50 dark:hover:bg-neutral-800/60'
          }`}>
          {t('skills.explorer.installedTab')}
          {skills.length > 0 && (
            <span className="ml-1.5 text-[10px] opacity-70">{skills.length}</span>
          )}
        </button>
      </div>

      {/* Search + format filter */}
      <div className="flex gap-2 px-1 pb-3">
        <div className="relative flex-1">
          <svg
            className="absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-stone-400 dark:text-neutral-500"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            strokeWidth={2}>
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="m21 21-5.197-5.197m0 0A7.5 7.5 0 1 0 5.196 5.196a7.5 7.5 0 0 0 10.607 10.607Z"
            />
          </svg>
          <input
            type="text"
            value={searchQuery}
            onChange={e => setSearchQuery(e.target.value)}
            placeholder={t('skills.explorer.searchPlaceholder')}
            className="w-full rounded-lg border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 py-2 pl-9 pr-3 text-xs text-stone-900 dark:text-neutral-100 placeholder:text-stone-400 dark:placeholder:text-neutral-500 shadow-sm transition-colors focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/30"
          />
        </div>
        {view === 'registry' && catalogFormats.length > 2 && (
          <select
            value={formatFilter}
            onChange={e => setFormatFilter(e.target.value)}
            className="rounded-lg border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 px-2 py-2 text-xs text-stone-700 dark:text-neutral-200 shadow-sm focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/30">
            {catalogFormats.map(f => (
              <option key={f} value={f}>
                {f === 'all' ? t('skills.explorer.allFormats') : f}
              </option>
            ))}
          </select>
        )}
        {view === 'registry' && (
          <button
            type="button"
            onClick={() => void fetchCatalog(true)}
            disabled={catalogLoading}
            title={t('skills.explorer.refreshRegistry')}
            className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 text-stone-500 dark:text-neutral-400 shadow-sm transition-colors hover:bg-stone-50 dark:hover:bg-neutral-800 disabled:opacity-50">
            <svg
              className={`h-4 w-4 ${catalogLoading ? 'animate-spin' : ''}`}
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              strokeWidth={2}>
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0 3.181 3.183a8.25 8.25 0 0 0 13.803-3.7M4.031 9.865a8.25 8.25 0 0 1 13.803-3.7l3.181 3.182"
              />
            </svg>
          </button>
        )}
      </div>

      {/* Loading */}
      {loading && (
        <div className="flex items-center justify-center py-12">
          <span className="h-5 w-5 animate-spin rounded-full border-2 border-stone-200 dark:border-neutral-700 border-t-primary-500" />
        </div>
      )}

      {/* Error */}
      {!loading && error && (
        <div className="mx-1 mb-3 rounded-xl border border-coral-200 dark:border-coral-500/30 bg-coral-50 dark:bg-coral-500/10 p-3">
          <p className="text-xs font-medium text-coral-700 dark:text-coral-300">{error}</p>
          <button
            type="button"
            onClick={() =>
              void (view === 'installed' ? fetchSkills() : fetchCatalog(true))
            }
            className="mt-2 rounded-lg border border-coral-200 dark:border-coral-500/30 px-3 py-1 text-[11px] font-medium text-coral-700 dark:text-coral-300 hover:bg-coral-100 dark:hover:bg-coral-500/20">
            {t('common.retry')}
          </button>
        </div>
      )}

      {/* ── Installed view ── */}
      {view === 'installed' && !loading && !error && (
        <>
          {skills.length === 0 && (
            <EmptyStateCard
              className="mx-1 mb-3 py-10"
              icon={
                <svg
                  className="h-7 w-7 text-primary-500"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  strokeWidth={1.5}>
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    d="M9.813 15.904 9 18.75l-.813-2.846a4.5 4.5 0 0 0-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 0 0 3.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 0 0 3.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 0 0-3.09 3.09Z"
                  />
                </svg>
              }
              title={t('skills.explorer.emptyTitle')}
              description={t('skills.explorer.emptyDescription')}
              actionLabel={t('skills.explorer.emptyCta')}
              onAction={() => setInstallDialogOpen(true)}
            />
          )}

          {skills.length > 0 && sortedSkills.length === 0 && (
            <p className="px-1 py-8 text-center text-xs text-stone-400 dark:text-neutral-500">
              {t('skills.noResults')}
            </p>
          )}

          {sortedSkills.length > 0 && (
            <div
              className="grid gap-2 sm:gap-3"
              style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(14rem, 1fr))' }}>
              {sortedSkills.map(skill => (
                <SkillTile
                  key={skill.id}
                  skill={skill}
                  onUninstall={() => setUninstallTarget(skill)}
                />
              ))}
            </div>
          )}
        </>
      )}

      {/* ── Registry view ── */}
      {view === 'registry' && !loading && !error && (
        <>
          {catalog.length === 0 && (
            <EmptyStateCard
              className="mx-1 mb-3 py-10"
              icon={
                <svg
                  className="h-7 w-7 text-primary-500"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  strokeWidth={1.5}>
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    d="M12 21v-8.25M15.75 21v-8.25M8.25 21v-8.25M3 9l9-6 9 6m-1.5 12V10.332A48.36 48.36 0 0 0 12 9.75c-2.551 0-5.056.2-7.5.582V21M3 21h18M12 6.75h.008v.008H12V6.75Z"
                  />
                </svg>
              }
              title={t('skills.explorer.registryEmptyTitle')}
              description={t('skills.explorer.registryEmptyDescription')}
              actionLabel={t('skills.explorer.refreshRegistry')}
              onAction={() => void fetchCatalog(true)}
            />
          )}

          {catalog.length > 0 && filteredCatalog.length === 0 && (
            <p className="px-1 py-8 text-center text-xs text-stone-400 dark:text-neutral-500">
              {t('skills.noResults')}
            </p>
          )}

          {filteredCatalog.length > 0 && (
            <div
              className="grid gap-2 sm:gap-3"
              style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(14rem, 1fr))' }}>
              {filteredCatalog.map(entry => (
                <CatalogTile
                  key={`${entry.source_id}-${entry.id}`}
                  entry={entry}
                  installed={installedIds.has(entry.id)}
                  installing={installingId === entry.id}
                  onInstall={() => void handleRegistryInstall(entry)}
                />
              ))}
            </div>
          )}
        </>
      )}

      {installDialogOpen && (
        <InstallSkillDialog
          onClose={() => setInstallDialogOpen(false)}
          onInstalled={handleInstalled}
        />
      )}

      {uninstallTarget && (
        <UninstallSkillConfirmDialog
          skill={uninstallTarget}
          onClose={() => setUninstallTarget(null)}
          onUninstalled={handleUninstalled}
        />
      )}
    </div>
  );
}
