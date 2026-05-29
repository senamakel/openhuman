import debug from 'debug';

import { callCoreRpc } from '../coreRpcClient';

const log = debug('workflowsApi');

/**
 * Scope a workflow was discovered in. Mirrors `WorkflowScope` on the Rust side
 * (serialized as a lowercase string).
 */
export type WorkflowScope = 'user' | 'project';

/** Tool-visibility scoping for a workflow or one of its phases. */
export interface ToolScope {
  allow: string[];
  deny: string[];
}

/** One lifecycle phase of a workflow. */
export interface WorkflowPhase {
  description?: string | null;
  rules: string[];
  scripts: string[];
  tools?: ToolScope | null;
  context: string[];
}

/** Catalog row returned by `openhuman.workflows_list`. */
export interface WorkflowSummary {
  id: string;
  name: string;
  description: string;
  when_to_use: string | null;
  tags: string[];
  /** Phase names declared by the workflow. */
  phases: string[];
  scope: WorkflowScope;
  location: string | null;
  warnings: string[];
}

/** Full workflow definition returned by `openhuman.workflows_read` / `_create`. */
export interface WorkflowDetail {
  id: string;
  name: string;
  description: string;
  when_to_use: string | null;
  tags: string[];
  tools?: ToolScope | null;
  /** Phase name → phase definition. */
  phases: Record<string, WorkflowPhase>;
  scope: WorkflowScope;
  location: string | null;
  warnings: string[];
}

/** Parameters accepted by `openhuman.workflows_create`. */
export interface CreateWorkflowInput {
  name: string;
  description: string;
  when_to_use?: string;
  scope?: WorkflowScope;
  tags?: string[];
}

/** Result of `openhuman.workflows_phase`. */
export interface WorkflowPhaseResult {
  workflow_id: string;
  phase: string;
  declared: boolean;
  guidance: string | null;
  tool_scope: ToolScope | null;
  context: string[];
  scripts: string[];
}

export interface UninstallWorkflowResult {
  name: string;
  removed_path: string;
  scope: WorkflowScope;
}

interface WorkflowsListResult {
  workflows: WorkflowSummary[];
}

interface WorkflowCreateResult {
  workflow: WorkflowDetail;
}

interface Envelope<T> {
  data?: T;
}

function unwrapEnvelope<T>(response: Envelope<T> | T): T {
  if (response && typeof response === 'object' && 'data' in response) {
    const envelope = response as Envelope<T>;
    if (envelope.data !== undefined) {
      return envelope.data as T;
    }
  }
  return response as T;
}

export const workflowsApi = {
  /** Enumerate WORKFLOW.md workflows visible in the active workspace. */
  listWorkflows: async (): Promise<WorkflowSummary[]> => {
    log('listWorkflows: request');
    const response = await callCoreRpc<Envelope<WorkflowsListResult> | WorkflowsListResult>({
      method: 'openhuman.workflows_list',
    });
    const result = unwrapEnvelope(response);
    const workflows = result?.workflows ?? [];
    log('listWorkflows: response count=%d', workflows.length);
    return workflows;
  },

  /** Read one workflow's full definition (all phases) by id. */
  readWorkflow: async (workflowId: string): Promise<WorkflowDetail> => {
    log('readWorkflow: request id=%s', workflowId);
    const response = await callCoreRpc<Envelope<WorkflowCreateResult> | WorkflowCreateResult>({
      method: 'openhuman.workflows_read',
      params: { workflow_id: workflowId },
    });
    return unwrapEnvelope(response).workflow;
  },

  /** Scaffold a new WORKFLOW.md workflow. */
  createWorkflow: async (input: CreateWorkflowInput): Promise<WorkflowDetail> => {
    log('createWorkflow: request name=%s', input.name);
    const response = await callCoreRpc<Envelope<WorkflowCreateResult> | WorkflowCreateResult>({
      method: 'openhuman.workflows_create',
      params: {
        name: input.name,
        description: input.description,
        when_to_use: input.when_to_use,
        scope: input.scope ?? 'user',
        tags: input.tags ?? [],
      },
    });
    return unwrapEnvelope(response).workflow;
  },

  /** Resolve a phase's guidance + effective tool scope for a workflow. */
  resolvePhase: async (workflowId: string, phase: string): Promise<WorkflowPhaseResult> => {
    log('resolvePhase: request id=%s phase=%s', workflowId, phase);
    const response = await callCoreRpc<Envelope<WorkflowPhaseResult> | WorkflowPhaseResult>({
      method: 'openhuman.workflows_phase',
      params: { workflow_id: workflowId, phase },
    });
    return unwrapEnvelope(response);
  },

  /** Remove a user-scope workflow by its on-disk slug. */
  uninstallWorkflow: async (name: string): Promise<UninstallWorkflowResult> => {
    log('uninstallWorkflow: request name=%s', name);
    const response = await callCoreRpc<Envelope<UninstallWorkflowResult> | UninstallWorkflowResult>(
      { method: 'openhuman.workflows_uninstall', params: { name } }
    );
    return unwrapEnvelope(response);
  },
};
