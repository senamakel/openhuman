# 08.4 — Per-agent graphs: stubs out, bespoke in

32 identical `agent_registry/agents/*/graph.rs` stubs return
`AgentGraph::Default` (~13 lines each). Delete the boilerplate; keep the
seam; land at least one real custom graph first so the cleanup doesn't
remove an unproven seam.

## Steps

1. Land the first bespoke graph — `orchestrator` (explicit plan/delegate/
   parallelize routing) or `researcher` (search → read → synthesize → cite,
   bounded + checkpointed) — as `AgentGraph::Custom` over a compiled graph
   with topology export. tool_maker (generate → validate → expose with
   review gates) is the third candidate.
2. Make `graph_fn` optional on `BuiltinAgent` (default = `AgentGraph::
   Default` supplied by the loader/registry); delete every stub `graph.rs`
   whose agent has no custom graph.
3. Registry diagnostics: `agent.graph_topologies` RPC already exists —
   extend it to show which graph (default/custom) each agent resolves to.

## Deletions

- All default-only `agent_registry/agents/*/graph.rs` files (~32 files /
  ~420 lines) + the `graph_fn` boilerplate wiring for them.

## Acceptance

- Registry loads all agents with correct graph resolution (test);
  at least one bespoke graph in production with route tests + topology
  snapshot.
