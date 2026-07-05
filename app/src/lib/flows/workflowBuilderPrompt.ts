/**
 * workflowBuilderPrompt (Phase 5c) — builds the natural-language turn text that
 * routes a chat turn to the `workflow_builder` specialist agent.
 *
 * There is no UI affordance to target a named agent for a turn: `chatSend`
 * carries only a thread + optional model/behaviour `profile_id`, and the core
 * always runs the turn through the orchestrator. The orchestrator's
 * `build_workflow` delegation edge routes any "build/automate/when-X-do-Y"
 * request to `workflow_builder` (see its `when_to_use` in
 * `agent_registry/agents/workflow_builder/agent.toml`). So instead of routing
 * directly, we phrase the turn so that delegation fires deterministically and
 * the specialist ends its turn by calling `propose_workflow` / `revise_workflow`
 * — the runtime then surfaces the returned proposal as a `WorkflowProposalCard`.
 *
 * Every builder here keeps the "propose, never persist" invariant: the prompts
 * ask for a PROPOSAL only. Saving/enabling stays behind the explicit
 * `WorkflowProposalCard` "Save & enable" click; nothing here can reach
 * `flows_create`/`flows_update`/`set_enabled`.
 */
import type { WorkflowGraph } from './types';

/** A leading directive that reliably trips the `build_workflow` delegation. */
const DELEGATE_DIRECTIVE =
  'Use the workflow builder to design a tinyflows automation and return a workflow proposal for me to review. Do not save, enable, or run anything.';

/**
 * Revise variant: still "propose, never persist" for saving/enabling, but the
 * copilot may run an ALREADY-SAVED flow to test it — only when I ask and after
 * confirming with me first (the specialist's own prompt enforces the ask).
 */
const DELEGATE_DIRECTIVE_REVISE =
  'Use the workflow builder to revise this tinyflows automation and return the revised proposal. Do not save or enable anything (I save via the UI). You may run_workflow the SAVED flow to test it, but ONLY if I ask and only after you confirm with me first.';

/** Serialize a graph compactly for injection as agent context. */
function serializeGraph(graph: WorkflowGraph): string {
  try {
    return JSON.stringify(graph);
  } catch {
    return '{}';
  }
}

/**
 * First-draft prompt for the Flows prompt bar. `description` is the user's
 * free-text ask ("email me a digest of new Slack messages every morning").
 */
export function buildCreatePrompt(description: string): string {
  const trimmed = description.trim();
  return `${DELEGATE_DIRECTIVE}\n\nBuild a workflow that does this:\n${trimmed}`;
}

/**
 * Iterative-refine prompt for the canvas copilot. Injects the CURRENT draft
 * graph so the specialist revises it in place (via `revise_workflow`) rather
 * than starting over. `instruction` is the user's change request ("add a Slack
 * notification on failure", "make the schedule weekdays only").
 */
export function buildRevisePrompt(
  instruction: string,
  graph: WorkflowGraph,
  flowId?: string | null
): string {
  const trimmed = instruction.trim();
  const lines = [
    DELEGATE_DIRECTIVE_REVISE,
    '',
    'Here is the current workflow draft (tinyflows WorkflowGraph JSON):',
    '```json',
    serializeGraph(graph),
    '```',
  ];
  if (flowId) {
    lines.push(
      '',
      `This workflow is saved with flow id \`${flowId}\` — if I ask you to run/test it, you may run_workflow that id, but confirm with me first.`
    );
  }
  lines.push('', 'Revise it as follows and return the full revised proposal:', trimmed);
  return lines.join('\n');
}

/** Context for a repair turn opened from a failed run's inspector. */
export interface RepairPromptContext {
  /** The failed run id (== thread_id) so the agent can `get_flow_run` it. */
  runId: string;
  /** The run-level error message, if any. */
  error?: string | null;
  /** Node ids that failed / are implicated, if known. */
  failingNodeIds?: string[];
  /** The flow's current graph, injected so the fix builds on the real draft. */
  graph: WorkflowGraph;
}

/**
 * Repair prompt for "Fix with agent". Preloads the failing run + step context
 * so the specialist reads the run (`get_flow_run`), diagnoses the failure, and
 * proposes a corrected graph.
 */
export function buildRepairPrompt(ctx: RepairPromptContext): string {
  const parts = [
    DELEGATE_DIRECTIVE,
    '',
    `A run of this workflow failed (run id: ${ctx.runId}). Read the run with get_flow_run, diagnose why it failed, and propose a fix.`,
  ];
  if (ctx.error && ctx.error.trim().length > 0) {
    parts.push('', `Run error: ${ctx.error.trim()}`);
  }
  if (ctx.failingNodeIds && ctx.failingNodeIds.length > 0) {
    parts.push('', `Failing step node id(s): ${ctx.failingNodeIds.join(', ')}`);
  }
  parts.push(
    '',
    'Here is the current workflow draft (tinyflows WorkflowGraph JSON):',
    '```json',
    serializeGraph(ctx.graph),
    '```',
    '',
    'Return the full corrected proposal.'
  );
  return parts.join('\n');
}
