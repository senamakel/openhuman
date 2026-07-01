# 08.5 — Worktrees behind `WorkspaceIsolation`

The crate has the isolation seam the sdk-gaps doc asked for:
`WorkspaceIsolation` trait, `WorkspaceDescriptor { root, trusted_roots,
policy_id, sandbox }` (+ `allows(path)`), `ToolExecutionContext.workspace`,
and `WorkspacePrepared/Violation/Cleanup` events.

## Steps

1. Implement `WorkspaceIsolation` over
   `agent_orchestration/worktree.rs` (git-worktree create/cleanup, dirty-
   worktree safeguards stay OpenHuman policy).
2. Thread `WorkspaceDescriptor` into tool execution: acting tools resolve
   their allowed root from `ToolExecutionContext.workspace` instead of
   task-local `worktree_context.rs`/action-dir globals. OpenHuman
   `SecurityPolicy` remains the enforcement authority — the descriptor is
   the carrier, not the policy.
3. Emit `WorkspacePrepared/Violation/Cleanup` through the bridge; violations
   also feed the security audit trail.
4. Sandbox descriptor: map `sandbox_mode` onto the descriptor's sandbox
   field so sandboxed runs are inspectable.

## Deletions

- `harness/worktree_context.rs` (74) task-local once tools read the
  descriptor; per-tool ad hoc root plumbing.

## Acceptance

- Parallel edit-capable workers run in isolated worktrees with descriptor-
  scoped tool roots; a path outside `allows()` is blocked AND evented;
  worktree tests (235) green.
