/**
 * useFlowRunProgress (Phase 3e — live run overlay)
 * ------------------------------------------------
 *
 * Subscribes to the core's live per-step progress feed for a single durable
 * `tinyflows` run and yields a `node_id -> status` map so the canvas can animate
 * nodes as they execute (n8n's signature running/success/error interaction).
 *
 * The backend's `FlowRunObserver` publishes `DomainEvent::FlowRunProgress` on
 * each finished step; the core socket bridge (`src/core/socketio.rs`) re-emits it
 * to the frontend as **both** `flow:run_progress` and `flow_run_progress`
 * (colon + underscore aliases, same as every other bridged event) with the
 * payload `{ run_id, node_id, status }`.
 *
 * This is a *live overlay only* — the durable `flow_runs` row remains the source
 * of truth and {@link useFlowRunPoller} stays as the 2s fallback, so a dropped
 * broadcast (lag) merely delays the animation, never corrupts run history. The
 * subscription mirrors {@link useTinyplaceStream} exactly (socketService.on/off
 * with cleanup on unmount / dependency change).
 */
import debug from 'debug';
import { useCallback, useEffect, useRef, useState } from 'react';

import { socketService } from '../services/socketService';

const log = debug('flows:run-progress');

/** Socket event aliases the core bridge emits (colon + underscore forms). */
const EVENT_COLON = 'flow:run_progress';
const EVENT_UNDERSCORE = 'flow_run_progress';

/** The per-item aliases, for nodes that fan out over their input. */
const ITEM_EVENT_COLON = 'flow:run_item_progress';
const ITEM_EVENT_UNDERSCORE = 'flow_run_item_progress';

/**
 * Node-level live status. The observer today emits only `success`/`error` on
 * step finish; `running` is included so the hook stays forward-compatible with
 * a future step-start event (and so callers can optimistically mark a node
 * active). Any unrecognized status string is passed through verbatim.
 */
export type FlowNodeRunStatus = 'running' | 'success' | 'error' | (string & {});

/** node_id → latest live status for the watched run. */
type FlowRunProgressMap = Record<string, FlowNodeRunStatus>;

/**
 * Maps a live node status to the canvas CSS class that rings/animates the node.
 * Kept here (not in the CSS-adjacent component) so the hook, the canvas, and
 * tests share one source of truth. `error` deliberately uses a run-specific
 * class distinct from validation's `.flow-node-error` so a *runtime* failure
 * reads differently from a *config* error.
 */
export const FLOW_RUN_NODE_STATUS_CLASS: Record<string, string> = {
  running: 'flow-node-running',
  success: 'flow-node-success',
  error: 'flow-node-failed',
  failed: 'flow-node-failed',
};

/**
 * How far a fanned-out node is through its batch.
 *
 * A node with `execution: "per_item"` and a `concurrency` is a *single* step
 * running N units of work, so the node-level status above can only say
 * "running" for the whole batch. This is the per-item breakdown, so the canvas
 * can show "3 / 8" and how many workers are live right now.
 */
export interface FlowNodeItemProgress {
  /** Batch size, known from the first frame. */
  total: number;
  /** Items currently started but not yet settled. */
  running: number;
  /** Items that completed successfully. */
  succeeded: number;
  /** Items whose own work failed (the node itself may still succeed). */
  failed: number;
}

/** node_id → per-item progress, for fanned-out nodes only. */
type FlowRunItemProgressMap = Record<string, FlowNodeItemProgress>;

interface FlowRunItemProgressPayload {
  run_id: string;
  node_id: string;
  index: number;
  total: number;
  status: string;
}

function parseItemPayload(data: unknown): FlowRunItemProgressPayload | null {
  if (!data || typeof data !== 'object') return null;
  const obj = data as Record<string, unknown>;
  if (typeof obj.run_id !== 'string') return null;
  if (typeof obj.node_id !== 'string') return null;
  if (typeof obj.index !== 'number') return null;
  if (typeof obj.total !== 'number') return null;
  if (typeof obj.status !== 'string') return null;
  return {
    run_id: obj.run_id,
    node_id: obj.node_id,
    index: obj.index,
    total: obj.total,
    status: obj.status,
  };
}

interface FlowRunProgressPayload {
  run_id: string;
  node_id: string;
  status: string;
}

function parsePayload(data: unknown): FlowRunProgressPayload | null {
  if (!data || typeof data !== 'object') return null;
  const obj = data as Record<string, unknown>;
  if (typeof obj.run_id !== 'string') return null;
  if (typeof obj.node_id !== 'string') return null;
  if (typeof obj.status !== 'string') return null;
  return { run_id: obj.run_id, node_id: obj.node_id, status: obj.status };
}

/**
 * Watch `runId`'s live progress. Returns a `node_id -> status` map that grows
 * as steps finish. Yields an empty map (and subscribes to nothing) when `runId`
 * is `null`. Resets whenever `runId` changes so a stale run's node states never
 * bleed onto a newly-started one.
 */
export function useFlowRunProgress(runId: string | null): FlowRunProgressMap {
  return useFlowRunProgressDetailed(runId).statuses;
}

/**
 * Like {@link useFlowRunProgress}, but also exposes per-item progress for
 * fanned-out nodes. Split so the common caller keeps its simple map return and
 * only the canvas pays for the extra state.
 */
export function useFlowRunProgressDetailed(runId: string | null): {
  statuses: FlowRunProgressMap;
  items: FlowRunItemProgressMap;
} {
  const [statuses, setStatuses] = useState<FlowRunProgressMap>({});
  const [items, setItems] = useState<FlowRunItemProgressMap>({});

  // Reset during render (not synchronously inside the effect below —
  // `react-hooks/set-state-in-effect` disallows that) when `runId` changes, so
  // a stale run's node states never bleed onto a newly-started one.
  const prevRunIdRef = useRef(runId);
  if (prevRunIdRef.current !== runId) {
    prevRunIdRef.current = runId;
    setStatuses({});
    setItems({});
  }

  const handleProgress = useCallback(
    (data: unknown) => {
      if (!runId) return;
      const payload = parsePayload(data);
      if (!payload) {
        log('progress: dropped — invalid payload %o', data);
        return;
      }
      // Filter to the run this hook instance is watching; the bridge broadcasts
      // every run's progress to all listeners.
      if (payload.run_id !== runId) return;
      log('progress: run=%s node=%s status=%s', runId, payload.node_id, payload.status);
      setStatuses(prev =>
        prev[payload.node_id] === payload.status
          ? prev
          : { ...prev, [payload.node_id]: payload.status }
      );
    },
    [runId]
  );

  const handleItemProgress = useCallback(
    (data: unknown) => {
      if (!runId) return;
      const payload = parseItemPayload(data);
      if (!payload) {
        log('item progress: dropped — invalid payload %o', data);
        return;
      }
      if (payload.run_id !== runId) return;
      setItems(prev => {
        const current = prev[payload.node_id] ?? {
          total: payload.total,
          running: 0,
          succeeded: 0,
          failed: 0,
        };
        // Counts, not per-index bookkeeping: the canvas shows "3 / 8" and a
        // live-worker count, and counting survives a dropped frame far more
        // gracefully than a per-index map would (which could strand an item as
        // permanently running).
        const next: FlowNodeItemProgress = { ...current, total: payload.total };
        if (payload.status === 'running') {
          next.running = current.running + 1;
        } else {
          next.running = Math.max(0, current.running - 1);
          if (payload.status === 'error') next.failed = current.failed + 1;
          else next.succeeded = current.succeeded + 1;
        }
        return { ...prev, [payload.node_id]: next };
      });
    },
    [runId]
  );

  useEffect(() => {
    if (!runId) return;
    log('subscribe: run=%s', runId);
    socketService.on(EVENT_COLON, handleProgress);
    socketService.on(EVENT_UNDERSCORE, handleProgress);
    socketService.on(ITEM_EVENT_COLON, handleItemProgress);
    socketService.on(ITEM_EVENT_UNDERSCORE, handleItemProgress);
    return () => {
      log('unsubscribe: run=%s', runId);
      socketService.off(EVENT_COLON, handleProgress);
      socketService.off(EVENT_UNDERSCORE, handleProgress);
      socketService.off(ITEM_EVENT_COLON, handleItemProgress);
      socketService.off(ITEM_EVENT_UNDERSCORE, handleItemProgress);
    };
  }, [runId, handleProgress, handleItemProgress]);

  return { statuses, items };
}
