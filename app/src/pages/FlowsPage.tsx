/**
 * FlowsPage — the Workflows list page (issue B5a).
 *
 * The discoverable hub for the `flows::` domain: lists every saved
 * `Flow` (name, enabled toggle, last-run status, Run button). This is NOT the
 * canvas (B5b ships flow authoring/editing) — until it lands, "New workflow"
 * (header + empty-state) bridges to the B4 agent-proposal flow in Chat
 * instead, since that's the only way to author a flow today.
 */
import createDebug from 'debug';
import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import EmptyStateCard from '../components/EmptyStateCard';
import FlowListRow, { type FlowListRowBusy } from '../components/flows/FlowListRow';
import FlowRunsDrawer from '../components/flows/FlowRunsDrawer';
import { ToastContainer } from '../components/intelligence/Toast';
import PanelPage from '../components/layout/PanelPage';
import Button from '../components/ui/Button';
import { CenteredLoadingState, ErrorBanner } from '../components/ui/LoadingState';
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

  /**
   * "New workflow" (there's no canvas builder yet — B5b) bridges to Chat so
   * the user can kick off B4's agent-proposal flow instead. There's no
   * existing mechanism to prefill or auto-send an initial composer message
   * from outside the Chat page — `Conversations.tsx` only reads
   * `location.state.openThreadId` (to reopen a thread), and the composer's
   * text is local `useState` with no Redux draft slice. This is the same gap
   * `ActionItemChecklist.tsx`'s "Run with OpenHuman" button already hit, so
   * we follow its precedent: navigate to `/chat` with no prefill rather than
   * build new prefill plumbing from scratch.
   */
  const handleNewWorkflow = useCallback(() => {
    log('new workflow: navigating to chat');
    // TODO: prefill the chat composer with a workflow-building prompt once a
    // draft/initial-message API exists (see ActionItemChecklist.tsx's
    // identical TODO for the same gap).
    navigate('/chat');
  }, [navigate]);

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

      <ToastContainer notifications={toasts} onRemove={removeToast} />
    </PanelPage>
  );
}
