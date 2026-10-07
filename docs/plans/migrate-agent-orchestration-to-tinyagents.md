# Extract `tinyagents-orchestration`

> **Status, verified 2026-10-07:** landed except O6, which would add
> `tinyagents-orchestration/src/parallel/`. See [`README.md`](README.md) for
> the whole set and how to re-check it.

Create the correctly spelled `vendor/tinyagents/crates/tinyagents-orchestration`
workspace crate for host-neutral *composition* of agent work. It is not a new
task registry, graph runtime, session database, tool package, or policy layer.

## Non-negotiable boundary

`tinyagents-orchestration` may depend on `tinyagents-harness`,
`tinyagents-graph`, and `tinyagents-session`. Those three crates must not depend
back on it. OpenHuman continues to depend directly on all four where it uses
their APIs. Do not add forwarding modules or re-export moved APIs from
`crate::agent::orchestration`.

Existing owners remain authoritative:

- graph: `DetachedTaskRegistry`, task stores/reconciliation/steering,
  DAG/map-reduce/workspace claims, checkpointing and delegation graph;
- harness: subagents, cancellation/steering, workspace interfaces and git
  worktrees;
- session: durable sessions, run ledger, team/workflow rows and CAS;
- OpenHuman: config, policy/security/approval, concrete execution adapters,
  BUS/progress/chat projection, persistence-root wiring, RPC schemas, tools,
  worktree policy and product prompt/catalog choices.

## Public API

The current crate exposes only the durable team ledger seam and callback member
graph, never OpenHuman values. A future workflow slice may add typed plans and
executor seams only when its engine consumes them:

```rust
pub trait TeamLedger: Send + Sync { /* team/member/task/event operations */ }

pub trait WorkflowStore: Send + Sync { /* load/upsert workflow snapshots */ }
#[async_trait]
pub trait WorkflowExecutor: Send + Sync {
    async fn execute(&self, request: WorkflowChildRequest,
        cancel: CancellationToken) -> Result<WorkflowChildResult, OrchestrationError>;
    async fn cancel_children(&self, child_ids: &[String]);
}
```

Provide `teams::{TeamService, TeamView, TeamError, run_member_graph}` and
`workflow::{WorkflowDefinition, WorkflowPhase, validate, WorkflowEngine,
WorkflowState, scheduler_graph}`. A `session-ledger` convenience adapter may
wrap `tinyagents_session::run_ledger` with a caller-supplied workspace path;
the host still chooses that path. Safety tier admission, agent lookup, tool
visibility and worktree choice are inputs already authorized by the host, not
crate decisions.

## Cargo files and graph

Add `crates/tinyagents-orchestration/Cargo.toml`, register it in
`vendor/tinyagents/Cargo.toml` default members, and use direct dependencies on
`anyhow`, `serde`, `serde_json`, `tokio`, `uuid`,
`tinyagents-harness`, `tinyagents-graph`, and `tinyagents-session`. Forward a
single optional `tracing` feature to the three TinyAgents crates. Do not add
`tinytools` unless a public signature needs it, and do not enable graph SQLite
for ordinary workflow state.

## File map

| OpenHuman source | Destination / action |
| --- | --- |
| `agent/orchestration/agent_teams/{types,ops,graph}.rs` | Move generic views/errors, dependency validation/service, and callback member graph to `tinyagents-orchestration/src/teams/`. |
| `agent/orchestration/agent_teams/runtime.rs` | Split generic claim/select/prompt/message-boundary/reconciliation engine to `teams/runtime.rs`; retain OpenHuman root-parent, `run_subagent`, config and projection adapter. |
| `agent/orchestration/workflow_runs/{types,graph}.rs`, pure `ops.rs` | Move to `src/workflow/{types,validate,graph}.rs`. Built-in host catalog and schema wiring stay. |
| `workflow_runs/engine/{state,scheduler,cancel}.rs` | Move to `src/workflow/`. |
| `workflow_runs/engine/{phase_exec,lifecycle}.rs` | Split generic bounded fanout/state machine to crate; keep `Config`, `AgentOrchestrationSession`, origin propagation, root parent, spawn and host persistence adapter. |
| `spawn_parallel_graph/request.rs` | Move only typed request validation to `src/parallel/request.rs`; do not move tool JSON parsing unless it becomes a typed boundary. |
| `spawn_parallel_graph/graph.rs` | Keep only a callback coordinator if it adds value; otherwise delete it and call graph `parallel::map_reduce` directly. |
| `spawn_parallel_graph/{types,staging,dispatch,workers,collect,run}.rs` | Keep OpenHuman: definition/tool-policy inspection, worktree prep, execution, AgentProgress and file-state projection. |
| `worktree.rs` aliases | Delete aliases and import `tinyagents_harness::workspace::{create_git_worktree, GitWorktreeBaseRef, ...}` directly. Keep only OpenHuman's BUS/policy-id adapter and schemas. |
| `ops.rs`, `types.rs`, `delegation.rs`, `parent_context/**`, `running_subagents/**`, `subagent_sessions/**`, `background_*`, `run_ledger_finalize.rs`, `subagent_events.rs`, `command_center/**`, `subagent_control.rs`, all `tools/**`, all RPC schemas | Keep in OpenHuman. |

## TDD slices

### O1 — crate skeleton and architecture gate

**RED:** Add a dependency-boundary test proving the new crate has no OpenHuman
dependency and no reverse edge from graph/harness/session. Add a compilation
test importing its empty public modules.

**GREEN:** Add the manifest, `lib.rs`, documentation and feature forwarding.
Verify `cargo metadata --manifest-path vendor/tinyagents/Cargo.toml --no-deps`
and `cargo test -p tinyagents-orchestration`.

### O2 — team service and member graph

**RED:** Port `agent_teams/graph_tests.rs` and generic `ops_tests.rs` cases to
the new crate with a fake `TeamLedger`: duplicate names, unknown
member/dependency, self/cycle rejection, ordered messages, claim race result,
quality-gated completion and recovery, non-claimant/owner-mismatch completion,
complete route, failed route, worker engine error, and optional graph-sink
delivery.

**GREEN:** Implement typed team service and callback graph. Add the optional
session-ledger adapter only after the trait suite is green.

### O3 — workflow definitions and scheduler

**RED:** Add fake-store/executor tests for duplicate/missing/cyclic/empty
phases, invalid concurrency, deterministic dependency order, upstream context,
synthesis fallback, scheduler topology and no-runnable failure.

**GREEN:** Move workflow definitions, validation, state helpers and scheduler
graph. Keep host safety-tier authorization outside the generic plan.

### O4 — workflow engine fanout, cancel and resume

**RED:** Port the behavior from `workflow_runs/engine_tests.rs`: bounded
concurrency, global child-cap failure, partial-output failure, cancellation
during a phase, terminal persistence, and resume skipping complete phases.

**GREEN:** Implement `WorkflowEngine<S,E>` using graph `map_reduce` and a
`CancellationToken`; it returns a future for the host to spawn in its own
origin/parent context.

### O5 — OpenHuman adapters and direct import cutover

**RED:** Preserve OpenHuman integration tests for team worker execution,
workflow policy denial, BUS/progress lifecycle, durable run rows and RPC wire
shapes. Add grep/checker coverage rejecting moved API re-exports.

**GREEN:** Implement minimal host adapters, migrate direct consumers and
topology registration, delete moved code and aliases in the same change.

### O6 — parallel/worktree cleanup

**RED:** Preserve `spawn_parallel_agents` policy tests and dirty-worktree
removal/refusal tests.

**GREEN:** Move only typed neutral request logic; replace all worktree aliases
with direct harness imports. Keep policy/worktree/BUS behavior host-side.

## Verification and PR order

For each TinyAgents slice run:

```bash
cargo test --manifest-path vendor/tinyagents/Cargo.toml -p tinyagents-orchestration
cargo test --manifest-path vendor/tinyagents/Cargo.toml -p tinyagents-graph -p tinyagents-session
cargo check --manifest-path vendor/tinyagents/Cargo.toml --workspace
```

For the host cutover run `pnpm debug rust agent_team`, `pnpm debug rust
workflow_run`, `pnpm debug rust spawn_parallel_agents`, `cargo check
--manifest-path Cargo.toml`, `pnpm rust:layout`, and `pnpm docs:check`. Use
`scripts/ci-cancel-aware.sh` for long workspace gates and never export
`CARGO_TARGET_DIR`.

Land in order: (1) session migration PR where its API is needed; (2) TinyAgents
new-crate PR; (3) OpenHuman gitlink update and direct-import cutover PR. Each
submodule branch/PR is against canonical upstream before the parent gitlink is
published; no nested worktrees, squashing, aliases or shims.
