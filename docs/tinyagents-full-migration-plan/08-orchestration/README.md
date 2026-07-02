# 08 — Orchestration & graphs

Finish graph adoption: parallel fanout on the SDK helper, spawn_parallel as
a graph tool, durable interrupts for approvals, workspace isolation hooks,
and graph-stub cleanup.

Current status (2026-07-02): graph adapter internals such as the delegation
state machine and production delegation glue stay crate-internal; product
orchestration modules own the public RPC/tool surfaces and output shapes.

Target SDK surface: `graph::parallel::map_reduce` + `ParallelOptions`/
`FailurePolicy`/`ItemOutcome`, `Send`/`Command`/reducers/`ChannelSet`,
`Interrupt`/`ResumeTarget`/`Command::resume`, `WorkspaceIsolation`/
`WorkspaceDescriptor` + workspace events, `GraphTopology` export,
graph testkit.

Steps:

1. `01-map-reduce.md` — replace `run_parallel_fanout` with the SDK helper.
2. `02-spawn-parallel-graph.md` — spawn_parallel_agents as validate→
   dispatch→worker→collect→finalize.
3. `03-interrupt-resume.md` — approval pauses as durable interrupts.
4. `04-graph-stubs.md` — delete the 32 default `graph.rs` stubs; land
   bespoke graphs.
5. `05-worktree-isolation.md` — worktrees behind `WorkspaceIsolation`.

Done when: fanout/parallel code paths are SDK-owned with deterministic
order + failure policy; every long-running orchestration has named nodes,
topology export, cancellation checks; the boilerplate stubs are gone.

Keep (product): workflow_runs/agent_teams/command_center product state + RPC
shapes; delegate/orchestration tool output formats; worktree policy.
