import { useCallback, useEffect, useState } from 'react';

import { useT } from '../lib/i18n/I18nContext';
import {
  type CreateWorkflowInput,
  type WorkflowDetail,
  workflowsApi,
  type WorkflowSummary,
} from '../services/api/workflowsApi';

type LoadState = 'idle' | 'loading' | 'error';

const cardClass =
  'rounded-2xl border border-stone-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 p-4 shadow-soft';
const primaryButtonClass =
  'rounded-lg bg-primary-500 px-3 py-2 text-xs font-semibold text-white shadow-soft transition-colors hover:bg-primary-600 focus:outline-none focus:ring-2 focus:ring-primary-500 focus:ring-offset-1 disabled:opacity-50';
const secondaryButtonClass =
  'rounded-lg border border-stone-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 px-3 py-2 text-xs font-medium text-stone-700 dark:text-neutral-200 shadow-soft transition-colors hover:bg-stone-50 dark:hover:bg-neutral-800 focus:outline-none focus:ring-2 focus:ring-primary-500 focus:ring-offset-1';
const inputClass =
  'w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-primary-500';

/**
 * Agent Workflows page — list / create / inspect / delete phase-keyed
 * WORKFLOW.md playbooks. Mirrors the conventions of the Skills page but kept
 * self-contained (local state + `workflowsApi`, no Redux slice).
 */
export default function Workflows() {
  const { t } = useT();
  const [workflows, setWorkflows] = useState<WorkflowSummary[]>([]);
  const [state, setState] = useState<LoadState>('idle');
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  // Create form state.
  const [showCreate, setShowCreate] = useState(false);
  const [form, setForm] = useState<CreateWorkflowInput>({
    name: '',
    description: '',
    when_to_use: '',
  });
  const [creating, setCreating] = useState(false);

  // Detail + delete state.
  const [openId, setOpenId] = useState<string | null>(null);
  const [detail, setDetail] = useState<WorkflowDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setState('loading');
    setError(null);
    try {
      setWorkflows(await workflowsApi.listWorkflows());
      setState('idle');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setState('error');
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleCreate = useCallback(async () => {
    if (!form.name.trim() || !form.description.trim()) {
      return;
    }
    setCreating(true);
    setError(null);
    try {
      const created = await workflowsApi.createWorkflow({
        name: form.name.trim(),
        description: form.description.trim(),
        when_to_use: form.when_to_use?.trim() || undefined,
      });
      setStatus(t('workflows.created', 'Workflow created'));
      setShowCreate(false);
      setForm({ name: '', description: '', when_to_use: '' });
      await refresh();
      setOpenId(created.id);
      setDetail(created);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setCreating(false);
    }
  }, [form, refresh, t]);

  const handleOpen = useCallback(
    async (id: string) => {
      if (openId === id) {
        setOpenId(null);
        setDetail(null);
        return;
      }
      setOpenId(id);
      setDetail(null);
      setDetailLoading(true);
      try {
        setDetail(await workflowsApi.readWorkflow(id));
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setDetailLoading(false);
      }
    },
    [openId]
  );

  const handleDelete = useCallback(
    async (id: string) => {
      setError(null);
      try {
        await workflowsApi.uninstallWorkflow(id);
        setStatus(t('workflows.deleted', 'Workflow deleted'));
        setPendingDelete(null);
        if (openId === id) {
          setOpenId(null);
          setDetail(null);
        }
        await refresh();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [openId, refresh, t]
  );

  return (
    <div className="min-h-full">
      <div className="min-h-full flex flex-col">
        <div className="flex-1 flex items-start justify-center p-4 pt-6">
          <div className="w-full max-w-3xl space-y-4">
            <div className="flex items-center justify-between gap-2">
              <div className="min-w-0">
                <h1 className="text-base font-semibold text-stone-900 dark:text-neutral-100">
                  {t('workflows.title', 'Workflows')}
                </h1>
                <p className="text-xs text-stone-500 dark:text-neutral-400">
                  {t(
                    'workflows.subtitle',
                    'Phase-keyed playbooks that steer how the agent approaches a task.'
                  )}
                </p>
              </div>
              <button
                type="button"
                onClick={() => setShowCreate(v => !v)}
                className={primaryButtonClass}>
                {t('workflows.newWorkflow', 'New workflow')}
              </button>
            </div>

            {status && (
              <div className="rounded-lg border border-sage-200 bg-sage-50 px-3 py-2 text-xs text-sage-800 dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-200">
                {status}
              </div>
            )}
            {error && (
              <div className="rounded-lg border border-coral-200 bg-coral-50 px-3 py-2 text-xs text-coral-800 dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-200">
                {error}
              </div>
            )}

            {showCreate && (
              <div className={cardClass}>
                <div className="space-y-3">
                  <label className="block">
                    <span className="mb-1 block text-xs font-medium text-stone-600 dark:text-neutral-300">
                      {t('workflows.field.name', 'Name')}
                    </span>
                    <input
                      className={inputClass}
                      value={form.name}
                      onChange={e => setForm(f => ({ ...f, name: e.target.value }))}
                      placeholder={t('workflows.field.namePlaceholder', 'e.g. Bug triage')}
                    />
                  </label>
                  <label className="block">
                    <span className="mb-1 block text-xs font-medium text-stone-600 dark:text-neutral-300">
                      {t('workflows.field.description', 'Description')}
                    </span>
                    <input
                      className={inputClass}
                      value={form.description}
                      onChange={e => setForm(f => ({ ...f, description: e.target.value }))}
                      placeholder={t(
                        'workflows.field.descriptionPlaceholder',
                        'What this workflow is for'
                      )}
                    />
                  </label>
                  <label className="block">
                    <span className="mb-1 block text-xs font-medium text-stone-600 dark:text-neutral-300">
                      {t('workflows.field.whenToUse', 'When to use')}
                    </span>
                    <input
                      className={inputClass}
                      value={form.when_to_use ?? ''}
                      onChange={e => setForm(f => ({ ...f, when_to_use: e.target.value }))}
                      placeholder={t(
                        'workflows.field.whenToUsePlaceholder',
                        'e.g. a user reports a bug'
                      )}
                    />
                  </label>
                  <div className="flex items-center justify-end gap-2">
                    <button
                      type="button"
                      onClick={() => setShowCreate(false)}
                      className={secondaryButtonClass}>
                      {t('common.cancel', 'Cancel')}
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleCreate()}
                      disabled={creating || !form.name.trim() || !form.description.trim()}
                      className={primaryButtonClass}>
                      {creating
                        ? t('workflows.creating', 'Creating…')
                        : t('workflows.create', 'Create')}
                    </button>
                  </div>
                </div>
              </div>
            )}

            {state === 'loading' && workflows.length === 0 && (
              <div className="text-xs text-stone-500 dark:text-neutral-400">
                {t('common.loading', 'Loading…')}
              </div>
            )}

            {state !== 'loading' && workflows.length === 0 && (
              <div className={`${cardClass} text-center`}>
                <p className="text-sm text-stone-600 dark:text-neutral-300">
                  {t(
                    'workflows.empty',
                    'No workflows yet. Create one to guide how the agent works.'
                  )}
                </p>
              </div>
            )}

            <ul className="space-y-3">
              {workflows.map(wf => (
                <li key={wf.id} className={cardClass}>
                  <div className="flex items-start justify-between gap-3">
                    <button
                      type="button"
                      onClick={() => void handleOpen(wf.id)}
                      className="min-w-0 flex-1 text-left">
                      <h2 className="truncate text-sm font-semibold text-stone-900 dark:text-neutral-100">
                        {wf.name}
                      </h2>
                      <p className="truncate text-xs text-stone-500 dark:text-neutral-400">
                        {wf.description}
                      </p>
                      <div className="mt-2 flex flex-wrap gap-1">
                        <span className="rounded-full bg-stone-100 px-2 py-0.5 text-[10px] font-medium text-stone-600 dark:bg-neutral-800 dark:text-neutral-300">
                          {wf.scope}
                        </span>
                        {wf.phases.map(phase => (
                          <span
                            key={phase}
                            className="rounded-full bg-primary-50 px-2 py-0.5 text-[10px] font-medium text-primary-700 dark:bg-neutral-800 dark:text-primary-300">
                            {phase}
                          </span>
                        ))}
                      </div>
                    </button>
                    {wf.scope === 'user' &&
                      (pendingDelete === wf.id ? (
                        <div className="flex flex-shrink-0 items-center gap-1">
                          <button
                            type="button"
                            onClick={() => void handleDelete(wf.id)}
                            className="rounded-lg bg-coral-500 px-2 py-1 text-[11px] font-semibold text-white hover:bg-coral-600">
                            {t('workflows.confirmDelete', 'Delete')}
                          </button>
                          <button
                            type="button"
                            onClick={() => setPendingDelete(null)}
                            className={secondaryButtonClass}>
                            {t('common.cancel', 'Cancel')}
                          </button>
                        </div>
                      ) : (
                        <button
                          type="button"
                          onClick={() => setPendingDelete(wf.id)}
                          className={secondaryButtonClass}
                          aria-label={t('workflows.delete', 'Delete workflow')}>
                          {t('workflows.delete', 'Delete')}
                        </button>
                      ))}
                  </div>

                  {openId === wf.id && (
                    <div className="mt-3 border-t border-stone-100 pt-3 dark:border-neutral-800">
                      {detailLoading && (
                        <p className="text-xs text-stone-500 dark:text-neutral-400">
                          {t('common.loading', 'Loading…')}
                        </p>
                      )}
                      {detail && detail.id === wf.id && <PhaseList detail={detail} />}
                    </div>
                  )}
                </li>
              ))}
            </ul>
          </div>
        </div>
      </div>
    </div>
  );
}

function PhaseList({ detail }: { detail: WorkflowDetail }) {
  const { t } = useT();
  const phaseNames = Object.keys(detail.phases);
  if (phaseNames.length === 0) {
    return (
      <p className="text-xs text-stone-500 dark:text-neutral-400">
        {t('workflows.noPhases', 'This workflow declares no phases.')}
      </p>
    );
  }
  return (
    <div className="space-y-3">
      {detail.when_to_use && (
        <p className="text-xs text-stone-500 dark:text-neutral-400">
          <span className="font-medium">{t('workflows.field.whenToUse', 'When to use')}:</span>{' '}
          {detail.when_to_use}
        </p>
      )}
      {phaseNames.map(name => {
        const phase = detail.phases[name];
        return (
          <div key={name}>
            <h3 className="font-mono text-xs font-semibold text-primary-700 dark:text-primary-300">
              {name}
            </h3>
            {phase.rules.length > 0 && (
              <ul className="ml-4 list-disc text-xs text-stone-600 dark:text-neutral-300">
                {phase.rules.map((rule, i) => (
                  <li key={i}>{rule}</li>
                ))}
              </ul>
            )}
            {phase.scripts.length > 0 && (
              <div className="ml-4 mt-1 text-xs text-stone-600 dark:text-neutral-300">
                <span className="font-medium">{t('workflows.scripts', 'Scripts')}:</span>
                <ul className="ml-4 list-disc">
                  {phase.scripts.map((script, i) => (
                    <li key={i} className="font-mono">
                      {script}
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {phase.context.length > 0 && (
              <p className="ml-4 text-xs text-stone-500 dark:text-neutral-400">
                {t('workflows.context', 'Context')}: {phase.context.join(', ')}
              </p>
            )}
          </div>
        );
      })}
    </div>
  );
}
