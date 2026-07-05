/**
 * FlowsPage — the Workflows list page (issue B5a).
 *
 * The discoverable hub for the `flows::` domain: lists every saved
 * `Flow` (name, enabled toggle, last-run status, Run button). "New workflow"
 * (header + empty-state) opens the Phase 4a chooser — start from scratch, pick
 * a template (Phase 4c), or describe it in Chat — each of which creates a flow
 * and opens the editable canvas (`/flows/:id`). The empty state also surfaces
 * the template gallery inline so first-time users have a one-click starting
 * point.
 */
import createDebug from 'debug';
import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import EmptyStateCard from '../components/EmptyStateCard';
import FlowListRow, { type FlowListRowBusy } from '../components/flows/FlowListRow';
import FlowRunsDrawer from '../components/flows/FlowRunsDrawer';
import FlowTemplateGallery from '../components/flows/FlowTemplateGallery';
import NewWorkflowModal from '../components/flows/NewWorkflowModal';
import { useCreateFlow } from '../components/flows/useCreateFlow';
import { ToastContainer } from '../components/intelligence/Toast';
import PanelPage from '../components/layout/PanelPage';
import Button from '../components/ui/Button';
import { CenteredLoadingState, ErrorBanner } from '../components/ui/LoadingState';
import { type FlowTemplate, templateNameKey } from '../lib/flows/templates';
import { useT } from '../lib/i18n/I18nContext';
import { type Flow, listFlows, runFlow, setFlowEnabled } from '../services/api/flowsApi';
import type { ToastNotification } from '../types/intelligence';

const log = createDebug('app:flows');

/** Which single row + action currently has a request in flight, if any. */
type BusyKey = `toggle:${string}` | `run:${string}`;

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export default function FlowsPage() {
  const { t } = useT();
  const navigate = useNavigate();
  const [flows, setFlows] = useState<Flow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<BusyKey | null>(null);
  const [toasts, setToasts] = useState<ToastNotification[]>([]);
  // Flow whose run history is open in `FlowRunsDrawer` (B3b's run inspector
  // then stacks on top of that when a specific run is picked). `null` keeps
  // the drawer unmounted.
  const [selectedFlowId, setSelectedFlowId] = useState<string | null>(null);
  // Whether the Phase 4a "New workflow" chooser modal is open.
  const [chooserOpen, setChooserOpen] = useState(false);
  // Create-and-open logic for the empty-state inline template gallery. (The
  // chooser modal owns its own `useCreateFlow` instance.)
  const emptyCreate = useCreateFlow();

  const addToast = useCallback((toast: Omit<ToastNotification, 'id'>) => {
    setToasts(prev => [...prev, { ...toast, id: `toast-${Date.now()}-${Math.random()}` }]);
  }, []);
  const removeToast = useCallback((id: string) => {
    setToasts(prev => prev.filter(item => item.id !== id));
  }, []);

  const loadFlows = useCallback(async () => {
    log('loading flows');
    setLoading(true);
    setError(null);
    try {
      const result = await listFlows();
      setFlows(result);
      log('loaded %d flows', result.length);
    } catch (err) {
      log('load failed: %o', err);
      setError(t('flows.page.loadError'));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void loadFlows();
  }, [loadFlows]);

  const handleToggle = useCallback(
    async (flow: Flow) => {
      if (busyKey) return;
      const key: BusyKey = `toggle:${flow.id}`;
      setBusyKey(key);
      setError(null);
      log('toggle: id=%s next=%s', flow.id, !flow.enabled);
      try {
        const updated = await setFlowEnabled(flow.id, !flow.enabled);
        setFlows(prev => prev.map(f => (f.id === updated.id ? updated : f)));
      } catch (err) {
        log('toggle failed: id=%s err=%o', flow.id, err);
        setError(errorMessage(err));
      } finally {
        setBusyKey(null);
      }
    },
    [busyKey]
  );

  const handleRun = useCallback(
    async (flow: Flow) => {
      if (busyKey) return;
      const key: BusyKey = `run:${flow.id}`;
      setBusyKey(key);
      setError(null);
      log('run: id=%s', flow.id);
      try {
        // Fire-and-forget: the caller doesn't wait for the run to finish,
        // just that it kicked off. The refetch below picks up the refreshed
        // `last_run_at` / `last_status` once the engine settles (or, for a
        // still-running flow, on the next manual refresh). Only refetch on
        // success — `loadFlows()` clears `error`, which would otherwise wipe
        // the failure banner set in the `catch` below.
        await runFlow(flow.id);
        addToast({ type: 'success', title: t('flows.list.runStarted') });
        await loadFlows();
      } catch (err) {
        log('run failed: id=%s err=%o', flow.id, err);
        setError(errorMessage(err));
      } finally {
        setBusyKey(null);
      }
    },
    [busyKey, addToast, loadFlows, t]
  );

  const busyFor = (flow: Flow): FlowListRowBusy => {
    if (busyKey === `toggle:${flow.id}`) return 'toggle';
    if (busyKey === `run:${flow.id}`) return 'run';
    return null;
  };

  const handleViewRuns = useCallback((flow: Flow) => {
    log('view runs: id=%s', flow.id);
    setSelectedFlowId(flow.id);
  }, []);

  /** Opens the read-only Workflow Canvas for this flow (issue B5b.1). */
  const handleView = useCallback(
    (flow: Flow) => {
      log('view: navigating to canvas id=%s', flow.id);
      navigate(`/flows/${flow.id}`);
    },
    [navigate]
  );

  const selectedFlow = flows.find(f => f.id === selectedFlowId) ?? null;

  /** "New workflow" opens the Phase 4a chooser (scratch / template / describe). */
  const handleNewWorkflow = useCallback(() => {
    log('new workflow: opening chooser');
    setChooserOpen(true);
  }, []);

  /**
   * "Describe it" hand-off: navigate to Chat so the user can invoke
   * `propose_workflow`. There's no mechanism yet to prefill/auto-send an
   * initial composer message from outside Chat (`Conversations.tsx` only reads
   * `location.state.openThreadId`, and the composer text is local `useState`
   * with no Redux draft slice — the same gap `ActionItemChecklist.tsx` hit), so
   * we navigate with no prefill.
   *
   * TODO(phase-5): replace this Chat hand-off with the in-place prompt bar that
   * runs `propose_workflow` directly on the canvas.
   */
  const handleDescribe = useCallback(() => {
    log('new workflow: describe — navigating to chat');
    setChooserOpen(false);
    navigate('/chat');
  }, [navigate]);

  /** Create a flow from an empty-state gallery card and open its canvas. */
  const handleEmptyTemplate = useCallback(
    (template: FlowTemplate) => {
      log('empty-state template selected: id=%s', template.id);
      void emptyCreate.create(template.id, t(templateNameKey(template.id)), template.graph);
    },
    [emptyCreate, t]
  );

  return (
    <PanelPage
      testId="flows-page"
      title={t('flows.page.title')}
      description={t('flows.page.description')}
      action={
        <Button
          type="button"
          variant="primary"
          size="sm"
          data-testid="flows-new-workflow"
          onClick={handleNewWorkflow}>
          {t('flows.page.newWorkflow')}
        </Button>
      }>
      <div className="mx-auto w-full max-w-3xl space-y-4">
        {error && (
          <div data-testid="flows-error">
            <ErrorBanner message={error} />
          </div>
        )}

        {loading && <CenteredLoadingState label={t('flows.page.loading')} />}

        {!loading && flows.length === 0 && !error && (
          <div className="space-y-4">
            <EmptyStateCard
              icon={
                <svg
                  className="h-7 w-7 text-primary-500"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  strokeWidth={1.5}>
                  <circle cx="5" cy="6" r="2" />
                  <circle cx="5" cy="18" r="2" />
                  <circle cx="19" cy="12" r="2" />
                  <path strokeLinecap="round" d="M7 6h4a4 4 0 014 4M7 18h4a4 4 0 004-4" />
                </svg>
              }
              title={t('flows.page.emptyTitle')}
              description={t('flows.page.emptyDescription')}
              actionLabel={t('flows.page.newWorkflow')}
              actionTestId="flows-empty-new-workflow"
              onAction={handleNewWorkflow}
            />

            <section className="space-y-3" data-testid="flows-empty-templates">
              <div>
                <h3 className="text-sm font-semibold text-content">{t('flows.templates.title')}</h3>
                <p className="text-xs text-content-muted">{t('flows.templates.subtitle')}</p>
              </div>
              {emptyCreate.error && (
                <div data-testid="flows-empty-template-error">
                  <ErrorBanner message={emptyCreate.error} />
                </div>
              )}
              <FlowTemplateGallery onSelect={handleEmptyTemplate} busyId={emptyCreate.busyKey} />
            </section>
          </div>
        )}

        {!loading && flows.length > 0 && (
          <div
            data-testid="flows-list"
            className="overflow-hidden rounded-2xl border border-line bg-surface">
            {flows.map(flow => (
              <FlowListRow
                key={flow.id}
                flow={flow}
                busy={busyFor(flow)}
                onToggle={f => void handleToggle(f)}
                onRun={f => void handleRun(f)}
                onViewRuns={handleViewRuns}
                onView={handleView}
              />
            ))}
          </div>
        )}
      </div>

      <FlowRunsDrawer
        flowId={selectedFlowId}
        flowName={selectedFlow?.name}
        onClose={() => setSelectedFlowId(null)}
      />

      {chooserOpen && (
        <NewWorkflowModal onClose={() => setChooserOpen(false)} onDescribe={handleDescribe} />
      )}

      <ToastContainer notifications={toasts} onRemove={removeToast} />
    </PanelPage>
  );
}
