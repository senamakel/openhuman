/**
 * FlowCanvasPage (issue B5b / Phase 3) — the Workflow Canvas builder at
 * `/flows/:id`. Loads one saved flow via `flows_get`, converts its
 * `WorkflowGraph` (`Flow.graph`, opaque `unknown` on the wire type — see
 * `services/api/flowsApi.ts`) to xyflow's shape via `graphAdapter.ts`, and
 * renders it in the *editable* `FlowCanvas` (drag / connect / add / delete /
 * config, plus Phase 3c validation UX and Phase 3d draft/dirty state).
 *
 * This page owns the two host-level pieces of Phase 3d the canvas can't:
 *  - **Save persistence** — `onSave` runs `flows_update(id, { graph })`. NO
 *    autosave: a saved+enabled flow is live, so an accidental save would fire
 *    real schedules. Save is only ever the explicit button in the canvas.
 *  - **Unsaved-changes guard** — the canvas reports its dirty state up via
 *    `onDirtyChange`; while dirty we (a) warn on a hard tab close/reload via
 *    `beforeunload`, and (b) intercept the in-page Back button with a confirm
 *    dialog. (App-wide route interception would need a data router; this app
 *    mounts a `HashRouter`, so full `useBlocker` interception isn't available —
 *    the Back button is this page's only in-app navigation affordance.)
 */
import createDebug from 'debug';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';

import FlowCanvas from '../components/flows/canvas/FlowCanvas';
import PanelPage from '../components/layout/PanelPage';
import Button from '../components/ui/Button';
import { CenteredLoadingState, ErrorBanner } from '../components/ui/LoadingState';
import { workflowGraphToXyflow } from '../lib/flows/graphAdapter';
import type { WorkflowGraph } from '../lib/flows/types';
import { useT } from '../lib/i18n/I18nContext';
import { type Flow, getFlow, updateFlow } from '../services/api/flowsApi';

const log = createDebug('app:flows:canvas');

type LoadState =
  | { status: 'loading' }
  | { status: 'notFound' }
  | { status: 'error'; message: string }
  | { status: 'ready'; flow: Flow };

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function BackIcon() {
  return (
    <svg
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
      aria-hidden="true">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
    </svg>
  );
}

/** The editable canvas body — split out so its hooks only mount once a flow loads. */
function FlowEditor({ flow }: { flow: Flow }) {
  const { t } = useT();
  const navigate = useNavigate();
  const [dirty, setDirty] = useState(false);
  const [leaveConfirm, setLeaveConfirm] = useState(false);

  const graph = flow.graph as WorkflowGraph;
  const { nodes, edges } = useMemo(() => workflowGraphToXyflow(graph), [graph]);
  const meta = useMemo(
    () => ({ schema_version: graph.schema_version, id: flow.id, name: flow.name }),
    [graph.schema_version, flow.id, flow.name]
  );

  // Persist the live graph via `flows_update`. Rejections propagate so the
  // canvas surfaces the failure inline (and leaves the draft dirty).
  const handleSave = useCallback(
    async (next: WorkflowGraph) => {
      log('save: flow id=%s nodes=%d edges=%d', flow.id, next.nodes.length, next.edges.length);
      await updateFlow(flow.id, { graph: next });
      log('save: flow id=%s persisted', flow.id);
    },
    [flow.id]
  );

  // Warn on hard tab close / reload while there are unsaved edits.
  useEffect(() => {
    if (!dirty) return;
    const handler = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [dirty]);

  const handleBack = useCallback(() => {
    if (dirty) {
      log('back: dirty — prompting for confirmation');
      setLeaveConfirm(true);
      return;
    }
    navigate('/flows');
  }, [dirty, navigate]);

  const backButton = (
    <Button
      type="button"
      variant="tertiary"
      size="xs"
      iconOnly
      data-testid="flow-canvas-back"
      aria-label={t('flows.canvas.backToList')}
      onClick={handleBack}>
      <BackIcon />
    </Button>
  );

  return (
    <PanelPage
      testId="flow-canvas-page"
      title={flow.name}
      leading={backButton}
      contentClassName="h-full p-0">
      <div className="relative h-full w-full">
        <FlowCanvas
          editable
          nodes={nodes}
          edges={edges}
          meta={meta}
          onSave={handleSave}
          onDirtyChange={setDirty}
        />

        {leaveConfirm && (
          <div
            className="absolute inset-0 z-30 flex items-center justify-center bg-black/30 p-4"
            data-testid="flow-leave-confirm">
            <div className="w-full max-w-sm rounded-xl border border-line bg-surface p-4 shadow-xl">
              <h2 className="text-sm font-semibold text-content">{t('flows.editor.leaveTitle')}</h2>
              <p className="mt-1 text-xs text-content-muted">{t('flows.editor.leaveBody')}</p>
              <div className="mt-4 flex justify-end gap-2">
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  data-testid="flow-leave-stay"
                  onClick={() => setLeaveConfirm(false)}>
                  {t('flows.editor.leaveStay')}
                </Button>
                <Button
                  type="button"
                  variant="primary"
                  tone="danger"
                  size="sm"
                  data-testid="flow-leave-discard"
                  onClick={() => {
                    log('back: confirmed leave — discarding unsaved edits');
                    navigate('/flows');
                  }}>
                  {t('flows.editor.leaveDiscard')}
                </Button>
              </div>
            </div>
          </div>
        )}
      </div>
    </PanelPage>
  );
}

export default function FlowCanvasPage() {
  const { t } = useT();
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const [state, setState] = useState<LoadState>({ status: 'loading' });

  useEffect(() => {
    // Guards a stale response from clobbering newer state: this effect
    // re-runs on every `:id` change without the component remounting (same
    // route, different param), and on unmount, so a slow fetch for a
    // previous id (or one that resolves after the component is gone) must
    // not call `setState` once superseded. Same pattern as
    // `useFlowRunPoller.ts`'s `cancelled`/`mountedRef` guard.
    let cancelled = false;

    if (!id) {
      log('load: no id in route params');
      setState({ status: 'notFound' });
      return;
    }

    log('load: fetching flow id=%s', id);
    setState({ status: 'loading' });

    void (async () => {
      try {
        const flow = await getFlow(id);
        if (cancelled) {
          log('load: fetched flow id=%s but superseded/unmounted, dropping', id);
          return;
        }
        log('load: fetched flow id=%s name=%s', flow.id, flow.name);
        setState({ status: 'ready', flow });
      } catch (err) {
        if (cancelled) return;
        const message = errorMessage(err);
        log('load: failed id=%s err=%o', id, err);
        if (message.toLowerCase().includes('not found')) {
          setState({ status: 'notFound' });
        } else {
          setState({ status: 'error', message });
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [id]);

  if (state.status === 'ready') {
    // Keyed by flow id so switching flows cleanly re-seeds the editable canvas's
    // controlled node/edge state (which only reads its props at mount).
    return <FlowEditor key={state.flow.id} flow={state.flow} />;
  }

  const backButton = (
    <Button
      type="button"
      variant="tertiary"
      size="xs"
      iconOnly
      data-testid="flow-canvas-back"
      aria-label={t('flows.canvas.backToList')}
      onClick={() => navigate('/flows')}>
      <BackIcon />
    </Button>
  );

  return (
    <PanelPage
      testId="flow-canvas-page"
      title={t('flows.canvas.title')}
      leading={backButton}
      contentClassName="h-full p-0">
      {state.status === 'loading' && (
        <div className="flex h-full items-center justify-center">
          <CenteredLoadingState label={t('flows.canvas.loading')} />
        </div>
      )}

      {state.status === 'error' && (
        <div className="p-4" data-testid="flow-canvas-error">
          <ErrorBanner message={state.message || t('flows.canvas.loadError')} />
        </div>
      )}

      {state.status === 'notFound' && (
        <div className="flex h-full items-center justify-center p-4">
          <p className="text-sm text-content-muted" data-testid="flow-canvas-not-found">
            {t('flows.canvas.notFound')}
          </p>
        </div>
      )}
    </PanelPage>
  );
}
