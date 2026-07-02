# 08.5 — Worktrees behind `WorkspaceIsolation`

The crate has the isolation seam the sdk-gaps doc asked for:
`WorkspaceIsolation` trait, `WorkspaceDescriptor { root, trusted_roots,
policy_id, sandbox }` (+ `allows(path)`), `ToolExecutionContext.workspace`,
and `WorkspacePrepared/Violation/Cleanup` events.

Current status (2026-07-02): do not delete
`src/openhuman/agent/harness/worktree_context.rs` yet. `spawn_parallel_agents`
still threads `worktree_action_dir` into the sub-agent runner, async subagent
session roots inherit the same override, and shell/git acting tools read the
task-local `current_action_dir_override()` to execute inside the isolated
checkout. The module is now hidden behind crate-internal harness re-exports.
Its task-local carrier and accessor helpers are no longer public beyond the
crate.
Delete it only after those tools resolve roots from a crate `WorkspaceDescriptor`
carried on `ToolExecutionContext`.

## Steps

1. Implement `WorkspaceIsolation` over
   `agent_orchestration/worktree.rs` (git-worktree create/cleanup, dirty-
   worktree safeguards stay OpenHuman policy).
2. Thread `WorkspaceDescriptor` into tool execution: TinyAgents owns the
   descriptor carrier, while acting tools resolve their allowed root from
   `ToolExecutionContext.workspace` instead of task-local
   `worktree_context.rs`/action-dir globals. OpenHuman `SecurityPolicy`
   remains the enforcement authority — the descriptor is the carrier, not the
   policy.
3. Emit `WorkspacePrepared/Violation/Cleanup` through the bridge; violations
   also feed the security audit trail. Use 1.3.0
   `WorkspaceDescriptor::enforce(path, events)` so the check and the
   violation event are one call.
4. Sandbox descriptor: map `sandbox_mode` onto the descriptor's sandbox
   field so sandboxed runs are inspectable.

## Deletions

- Later: `harness/worktree_context.rs` (74) task-local once shell/git and other
  acting tools read the descriptor; per-tool ad hoc root plumbing.

## Acceptance

- Parallel edit-capable workers run in isolated worktrees with descriptor-
  scoped tool roots; a path outside `allows()` is blocked AND evented;
  worktree tests (235) green.
