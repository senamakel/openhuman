# 10 — CapabilityRegistry projection

One policy-aware lookup over OpenHuman's many registries (agents, tools,
MCP, models, graphs).

Current status (2026-07-02): `agent.graph_topologies` still exposes the
structure-only graph snapshot as JSON-RPC, but the underlying
`tinyagents/topology.rs` helper/report surface is now crate-internal. A full
`CapabilityRegistry` projection remains pending.

Target SDK surface: `CapabilityRegistry` (`register_model/tool/
graph_blueprint/agent`, `to_model_registry()`, `to_tool_registry()`,
`capability_resolver()`), `ComponentId`/`ComponentKind`/
`ComponentMetadata { tags, aliases }`, `RegistrySnapshot` (`to_dot()`),
`RegistryDiagnostic`/`DiagnosticSeverity`, `ModelCatalog`.

## Steps

1. Build-per-run (or cached) `CapabilityRegistry` projection in
   `assemble_turn_harness`: models from 02.1 routes, tools from the
   turn's tool sets, graph descriptors from the current topology reports
   (delegation/scheduler/member/subagent-pipeline/spawn-parallel), agents from
   `AgentDefinitionRegistry` (built-ins plus workspace TOML overrides). `.rag`
   `Blueprint` registration comes later when OpenHuman has real blueprint
   artifacts; the current graph exports are structure snapshots, not compiled
   blueprints. `AgentHarness` does not yet expose a registry-replacement setter,
   so `to_model_registry()`/`to_tool_registry()` start as validation/projection
   helpers before they can replace the hand-rolled registration blocks.
2. Diagnostics fail-closed pre-dispatch: duplicate tool names across
   native/MCP/Composio/generated tools, unsafe aliases → registry
   diagnostic errors (today: duplicate handling is scattered across generated
   tools, MCP, and native registration instead of one SDK diagnostic stream).
   TinyAgents 1.3.0 is pinned and exposes `AliasBinding`, alias diagnostics,
   cross-kind name-reuse detection, and `ComponentKind::{Middleware,
   Checkpointer, TaskStore, Listener}`; OpenHuman still needs to project those
   SDK diagnostics into its runtime.
3. Introspection RPC: extend the existing `agent.graph_topologies` surface or
   add a sibling RPC for `RegistrySnapshot` (JSON + DOT) so a UI/CLI can show
   every active component.
4. This is also the enabler for `.rag` blueprints later (registry-bound
   capability resolution) — out of scope for this wave, note only.

## Deletions

- Hand-rolled duplicate-name checks scattered in tool assembly (moved to
  diagnostics); redundant registration glue in `mod.rs`.

## Acceptance

- Duplicate tool name → typed failure before model dispatch (test);
  snapshot RPC lists models/tools/graphs/agents with metadata;
  adapter-inventory test re-pointed at the registry.
