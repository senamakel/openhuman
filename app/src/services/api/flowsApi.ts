/**
 * Frontend client for the durable `openhuman.flows_*` run surface (issue B2 /
 * B3 / B4). Wraps the subset of controllers the B3a approval card, the B3b
 * run inspector, and the B4 agent-proposal card need:
 *   - `flows_create`    — persist a new flow (B4 — only ever called from the
 *     user's own "Save & enable" click on `WorkflowProposalCard`; the agent's
 *     `propose_workflow` tool only validates and never reaches this RPC)
 *   - `flows_resume`    — resume a `pending_approval` run past its checkpoint
 *   - `flows_list_runs` — recent runs for a flow, newest first (B3b)
 *   - `flows_get_run`   — a single run record by id (B3b)
 *
 * Wire shape note: every `src/openhuman/flows/ops.rs` handler returns its
 * value via `RpcOutcome::single_log(value, "...")`, which
 * `into_cli_compatible_json` ALWAYS wraps as `{ result: value, logs: [...] }`
 * (see `src/rpc/mod.rs`) because a log message is always attached. `callCoreRpc`
 * already unwraps the outer JSON-RPC envelope, so the value this client
 * receives is that `{ result, logs }` object — {@link unwrapCliEnvelope} peels
 * it back to the actual payload. This mirrors the private helper of the same
 * name in `channelConnectionsApi.ts`.
 *
 * `flows_resume`'s payload is NOT a `FlowRun` row — it's the raw tinyflows
 * resume outcome (`{ output, pending_approvals, thread_id }`). The persisted
 * `FlowRun` record (with `status`/`steps`/etc.) only comes back from
 * `flows_list_runs` / `flows_get_run`. The approval card only needs to know
 * the resume call succeeded, so it doesn't need the full row.
 */
import debug from 'debug';

import { callCoreRpc } from '../coreRpcClient';

const log = debug('flowsApi');

/**
 * `openhuman.flows_resume` and `openhuman.flows_run` both drive the tinyflows
 * engine and can run up to ~600s server-side (`FLOW_RUN_TIMEOUT_SECS` in
 * `src/openhuman/flows/ops.rs`). Give the client a slightly larger budget than
 * the default 30s so a slow run/resume doesn't fail client-side while the
 * engine is still running.
 */
const FLOW_RESUME_TIMEOUT_MS = 610_000;

// ---------------------------------------------------------------------------
// Wire types — mirror `src/openhuman/flows/types.rs`. No rename_all attribute
// on the Rust structs, so field names are snake_case on the wire as-is.
// ---------------------------------------------------------------------------

/** Lifecycle status of a durable flow run. */
export type FlowRunStatus = 'running' | 'completed' | 'pending_approval' | 'failed';

/** One reconstructed step of a persisted `FlowRun` (`src/openhuman/flows/types.rs::FlowRunStep`). */
export interface FlowRunStep {
  node_id: string;
  output: unknown;
  /** Output port the node routed on, if any (branching/switch nodes). Omitted when absent. */
  port?: string;
}

/** A persisted flow run record (`src/openhuman/flows/types.rs::FlowRun`). */
export interface FlowRun {
  /** Same value as `thread_id` (the tinyflows checkpointer key). */
  id: string;
  flow_id: string;
  thread_id: string;
  status: FlowRunStatus;
  started_at: string;
  finished_at?: string | null;
  steps: FlowRunStep[];
  /** Node ids paused awaiting approval when `status === 'pending_approval'`. */
  pending_approvals: string[];
  error?: string | null;
}

/**
 * Raw resume outcome returned by `openhuman.flows_resume` — the immediate
 * tinyflows engine result, not the persisted `FlowRun` row. Call
 * {@link getFlowRun} afterwards (thread_id === run id) if the caller needs the
 * up-to-date persisted status.
 */
export interface FlowResumeResult {
  output: unknown;
  pending_approvals: string[];
  thread_id: string;
}

/**
 * A saved automation workflow (`src/openhuman/flows/types.rs::Flow`) — the
 * Workflows list page (B5a) row shape. `graph` is the raw tinyflows
 * `WorkflowGraph`; the list page doesn't need to interpret it, only the
 * canvas (B5b) does, so it's kept as `unknown` here.
 */
export interface Flow {
  /** Stable identifier (UUID) for this flow. */
  id: string;
  /** Human-readable name shown in the Workflows UI. */
  name: string;
  /** Whether this flow may currently be triggered/run. */
  enabled: boolean;
  /** The validated, migrated workflow graph — opaque to this client. */
  graph: unknown;
  /** RFC3339 creation timestamp. */
  created_at: string;
  /** RFC3339 last-update timestamp. */
  updated_at: string;
  /** RFC3339 timestamp of the most recent run, if any. */
  last_run_at: string | null;
  /** Outcome of the most recent run: `"completed"` | `"pending_approval"` | `"failed"`. */
  last_status: string | null;
  /** "Require approval for outbound actions" toggle (issue B2). */
  require_approval: boolean;
}

// ---------------------------------------------------------------------------
// CLI-compatible envelope unwrapping.
// ---------------------------------------------------------------------------

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

/**
 * Every `flows_*` handler goes through `RpcOutcome::single_log`, so the value
 * `callCoreRpc` resolves is always `{ result: <payload>, logs: string[] }`.
 * Peel that back to `<payload>`. Falls through unchanged if the shape doesn't
 * match (defensive — keeps this client working if a future handler switches
 * to a log-less `RpcOutcome::new` and stops wrapping).
 */
function unwrapCliEnvelope<T>(payload: unknown): T {
  const record = asRecord(payload);
  if (record && 'result' in record && 'logs' in record && Array.isArray(record.logs)) {
    return record.result as T;
  }
  return payload as T;
}

// ---------------------------------------------------------------------------
// RPC client.
// ---------------------------------------------------------------------------

/**
 * Create (and, by default, enable) a new saved flow via `openhuman.flows_create`
 * (issue B4). This is the ONLY path that persists a flow — the agent's
 * `propose_workflow` tool (`src/openhuman/flows/tools.rs`) only validates a
 * candidate graph and returns a summary; `WorkflowProposalCard`'s "Save &
 * enable" button is what calls this function, directly from the client, on
 * the user's explicit action. `requireApproval` defaults server-side to
 * `false` when omitted, but the B4 proposal flow always passes it explicitly
 * (defaulting to `true` on the Rust tool side) so a saved agent-proposed flow
 * starts with its outbound-action approval gate on.
 */
export async function createFlow(
  name: string,
  graph: unknown,
  requireApproval?: boolean
): Promise<Flow> {
  log('createFlow: request name=%s requireApproval=%s', name, requireApproval ?? 'default');
  const response = await callCoreRpc<unknown>({
    method: 'openhuman.flows_create',
    params:
      requireApproval === undefined
        ? { name, graph }
        : { name, graph, require_approval: requireApproval },
  });
  const flow = unwrapCliEnvelope<Flow>(response);
  log('createFlow: response id=%s name=%s enabled=%s', flow.id, flow.name, flow.enabled);
  return flow;
}

/**
 * Resume a `pending_approval` flow run past its checkpoint via
 * `openhuman.flows_resume`. `approvals` should name the node ids from the
 * triggering notification's `node_ids` payload — the Rust side rejects the
 * call outright unless at least one named id matches a currently-pending gate.
 */
export async function resumeFlow(
  id: string,
  threadId: string,
  approvals: string[]
): Promise<FlowResumeResult> {
  log('resumeFlow: request id=%s threadId=%s approvals=%o', id, threadId, approvals);
  const response = await callCoreRpc<unknown>({
    method: 'openhuman.flows_resume',
    params: { id, thread_id: threadId, approvals },
    timeoutMs: FLOW_RESUME_TIMEOUT_MS,
  });
  const result = unwrapCliEnvelope<FlowResumeResult>(response);
  log(
    'resumeFlow: response threadId=%s pendingApprovals=%d',
    result.thread_id,
    result.pending_approvals?.length ?? 0
  );
  return result;
}

/**
 * List recent runs for a flow, newest first, via `openhuman.flows_list_runs`.
 * `limit` defaults to 20 server-side. Not used by the B3a approval card —
 * exported now for the B3b run-history inspector.
 */
export async function listFlowRuns(flowId: string, limit?: number): Promise<FlowRun[]> {
  log('listFlowRuns: request flowId=%s limit=%s', flowId, limit ?? 'default');
  const response = await callCoreRpc<unknown>({
    method: 'openhuman.flows_list_runs',
    params: limit === undefined ? { id: flowId } : { id: flowId, limit },
  });
  const runs = unwrapCliEnvelope<FlowRun[]>(response);
  log('listFlowRuns: response count=%d', runs.length);
  return runs;
}

/**
 * Load a single flow run record by id (== thread_id) via
 * `openhuman.flows_get_run`. Not used by the B3a approval card — exported now
 * for the B3b run-history inspector.
 */
export async function getFlowRun(runId: string): Promise<FlowRun> {
  log('getFlowRun: request runId=%s', runId);
  const response = await callCoreRpc<unknown>({
    method: 'openhuman.flows_get_run',
    params: { run_id: runId },
  });
  const run = unwrapCliEnvelope<FlowRun>(response);
  log('getFlowRun: response id=%s status=%s', run.id, run.status);
  return run;
}

/**
 * List all saved flows via `openhuman.flows_list` (the Workflows list page,
 * B5a). No params. Unlike the run-surface calls above, the payload IS the
 * `Flow[]` array directly — there is no outer `{ flows: [...] }` wrapper (see
 * `src/openhuman/flows/ops.rs::flows_list`, which returns `Vec<Flow>`
 * straight through `RpcOutcome::single_log`).
 */
export async function listFlows(): Promise<Flow[]> {
  log('listFlows: request');
  const response = await callCoreRpc<unknown>({ method: 'openhuman.flows_list', params: {} });
  const flows = unwrapCliEnvelope<Flow[]>(response);
  log('listFlows: response count=%d', flows.length);
  return flows;
}

/**
 * Enable or disable a saved flow via `openhuman.flows_set_enabled`. Returns
 * the updated `Flow` row directly (same no-wrapper shape as `flows_list`'s
 * elements).
 */
export async function setFlowEnabled(id: string, enabled: boolean): Promise<Flow> {
  log('setFlowEnabled: request id=%s enabled=%s', id, enabled);
  const response = await callCoreRpc<unknown>({
    method: 'openhuman.flows_set_enabled',
    params: { id, enabled },
  });
  const flow = unwrapCliEnvelope<Flow>(response);
  log('setFlowEnabled: response id=%s enabled=%s', flow.id, flow.enabled);
  return flow;
}

/**
 * Run a saved flow to completion (or until it pauses on a human-approval
 * gate) via `openhuman.flows_run`. This is the call that actually drives the
 * tinyflows engine, so it shares `flows_resume`'s ~600s server-side budget
 * (see {@link FLOW_RESUME_TIMEOUT_MS}). The Workflows list page's Run button
 * uses this fire-and-forget: it awaits the call just long enough to know the
 * run kicked off, shows a toast, and refetches `listFlows()` to pick up the
 * refreshed `last_run_at`/`last_status`.
 */
export async function runFlow(id: string, input?: unknown): Promise<FlowResumeResult> {
  log('runFlow: request id=%s', id);
  const response = await callCoreRpc<unknown>({
    method: 'openhuman.flows_run',
    params: { id, input: input ?? null },
    timeoutMs: FLOW_RESUME_TIMEOUT_MS,
  });
  const result = unwrapCliEnvelope<FlowResumeResult>(response);
  log(
    'runFlow: response threadId=%s pendingApprovals=%d',
    result.thread_id,
    result.pending_approvals?.length ?? 0
  );
  return result;
}

export const flowsApi = {
  createFlow,
  resumeFlow,
  listFlowRuns,
  getFlowRun,
  listFlows,
  setFlowEnabled,
  runFlow,
};

export default flowsApi;
