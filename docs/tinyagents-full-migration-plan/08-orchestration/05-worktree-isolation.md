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
`agent_orchestration::worktree::GitWorktreeIsolation` now implements the
TinyAgents `WorkspaceIsolation` trait over OpenHuman's existing git-worktree
create/remove policy and returns `WorkspaceDescriptor` roots for isolated
workers. Sub-agent run options now carry an optional `WorkspaceDescriptor`, and
the TinyAgents shared turn seam attaches it to `RunContext::with_workspace`.
For current `spawn_parallel_agents` worktree workers, the runner derives the
descriptor from the existing `worktree_action_dir` while keeping the old
task-local action-dir override as the live tool fallback.
The TinyAgents tool adapter now forwards `ToolExecutionContext` into OpenHuman
tools, and shell/git plus core filesystem tools (`file_read`, `list`,
`file_write`, `edit`) use `ToolExecutionContext.workspace.root` as their
effective action directory when present. For shell this also covers sandbox
policy and sandbox cwd. Delete the legacy task-local only after the remaining
acting tools also resolve roots from the carried crate `WorkspaceDescriptor`.

## Steps

1. Partially done: implement `WorkspaceIsolation` over
   `agent_orchestration/worktree.rs` (git-worktree create/cleanup, dirty-
   worktree safeguards stay OpenHuman policy). The adapter now exists; live
   callers still need to use `prepare_workspace`/`cleanup_workspace`.
2. In progress: thread `WorkspaceDescriptor` into tool execution. TinyAgents
   owns the descriptor carrier, while acting tools resolve their allowed root
   from `ToolExecutionContext.workspace` instead of task-local
   `worktree_context.rs`/action-dir globals. The carrier is now threaded
   through sub-agent run options, `RunContext::with_workspace`, the OpenHuman
   tool adapter, shell, git operations, `file_read`, `list`, `file_write`, and
   `edit`. Remaining acting tools still need to read it. OpenHuman
   `SecurityPolicy` remains the enforcement authority — the descriptor is the
   carrier, not the policy.
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
