/**
 * FlowCanvasPage — the read-only Workflow Canvas view (issue B5b.1) at
 * `/flows/:id`. Loads one saved flow via `flows_get`, converts its
 * `WorkflowGraph` (`Flow.graph`, opaque `unknown` on the wire type — see
 * `services/api/flowsApi.ts`) to xyflow's shape via `graphAdapter.ts`, and
 * renders it in `FlowCanvas` with editing disabled. This is the first slice
 * of the visual builder (de-risking the `@xyflow/react` integration) —
 * dragging nodes / drawing edges lands in B5b.2+.
 */
import createDebug from 'debug';
import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';

import FlowCanvas from '../components/flows/canvas/FlowCanvas';
import PanelPage from '../components/layout/PanelPage';
import Button from '../components/ui/Button';
import { CenteredLoadingState, ErrorBanner } from '../components/ui/LoadingState';
import { workflowGraphToXyflow } from '../lib/flows/graphAdapter';
import type { WorkflowGraph } from '../lib/flows/types';
import { useT } from '../lib/i18n/I18nContext';
import { type Flow, getFlow } from '../services/api/flowsApi';

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

  const title = state.status === 'ready' ? state.flow.name : t('flows.canvas.title');

  return (
    <PanelPage
      testId="flow-canvas-page"
      title={title}
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

      {state.status === 'ready' &&
        (() => {
          const graph = state.flow.graph as WorkflowGraph;
          const { nodes, edges } = workflowGraphToXyflow(graph);
          return <FlowCanvas nodes={nodes} edges={edges} readonly />;
        })()}
    </PanelPage>
  );
}
