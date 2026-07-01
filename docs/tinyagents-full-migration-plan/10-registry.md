# 10 — CapabilityRegistry projection

One policy-aware lookup over OpenHuman's many registries (agents, tools,
MCP, models, graphs).

Target SDK surface: `CapabilityRegistry` (`register_model/tool/
graph_blueprint/agent`, `to_model_registry()`, `to_tool_registry()`,
`capability_resolver()`), `ComponentId`/`ComponentKind`/
`ComponentMetadata { tags, aliases }`, `RegistrySnapshot` (`to_dot()`),
`RegistryDiagnostic`/`DiagnosticSeverity`, `ModelCatalog`.

## Steps

1. Build-per-run (or cached) `CapabilityRegistry` projection in
   `assemble_turn_harness`: models from 02.1 routes, tools from the
   turn's tool sets, compiled graphs (delegation/scheduler/member/
   subagent-pipeline/spawn-parallel), agents from `agent_registry`.
   `to_model_registry()`/`to_tool_registry()` replace the hand-rolled
   registration blocks.
2. Diagnostics fail-closed pre-dispatch: duplicate tool names across
   native/MCP/Composio/generated tools, unsafe aliases → registry
   diagnostic errors (today: silent shadowing risk in
   `tools/generated.rs` + `mcp_registry`).
3. Introspection RPC: expose `RegistrySnapshot` (JSON + DOT) alongside
   `agent.graph_topologies` so a UI/CLI can show every active component.
4. This is also the enabler for `.rag` blueprints later (registry-bound
   capability resolution) — out of scope for this wave, note only.

## Deletions

- Hand-rolled duplicate-name checks scattered in tool assembly (moved to
  diagnostics); redundant registration glue in `mod.rs`.

## Acceptance

- Duplicate tool name → typed failure before model dispatch (test);
  snapshot RPC lists models/tools/graphs/agents with metadata;
  adapter-inventory test re-pointed at the registry.
