# 01.1 — ToolPolicy round-trip

The spec's "SDK gap: ToolSchema has no metadata map" is obsolete: the crate
`Tool` trait has `policy() -> ToolPolicy` and `ToolRegistry::policies()`.

Current status (2026-07-02): adapters expose classified SDK policies, the
crate `ToolPolicyMiddleware` is installed from `harness.tools().policies()`, and
`ToolOutputMiddleware` now reads per-tool result caps from that registry
snapshot instead of rebuilding a `name -> Arc<dyn Tool>` lookup. The remaining
OpenHuman lookup in `ApprovalSecurityMiddleware` is the args-aware
`external_effect_with_args` overlay called out below.

## Steps

1. In `src/openhuman/tinyagents/tools.rs` (`ToolAdapter`/`SharedToolAdapter`),
   implement `policy()` by mapping OpenHuman trait methods
   (`src/openhuman/tools/traits.rs`): permission level → `ToolAccess
   { approval_required, background_safe }`; `external_effect` →
   `ToolSideEffects`; timeout policy → `ToolRuntime.timeout_ms`;
   concurrency safety → `ToolRuntime.idempotent`; `max_result_size_chars`
   → `ToolRuntime.max_result_bytes`; sandbox mode → `SandboxMode`.
   Every adapter-produced policy sets `classified: true`.
2. Partially done: rework `ApprovalSecurityMiddleware` and `ToolOutputMiddleware`
   (`src/openhuman/tinyagents/middleware.rs`) to read `ToolPolicy` from the
   registry instead of the shared `name → Arc<dyn Tool>` side-lookup.
   `ToolOutputMiddleware` is registry-backed. Keep args-aware checks
   (`external_effect_with_args`) as an OpenHuman overlay where the static policy
   is insufficient — document each overlay.
3. Install crate `ToolPolicyMiddleware` in `assemble_turn_harness`
   (`src/openhuman/tinyagents/mod.rs`) fail-closed, configured with the
   1.3.0 builders (`require_sandbox`/`require_approval`/
   `enforce_result_bytes`); assert every registered tool is classified in
   the adapter-inventory test.
4. Done for output budgeting: `TurnContextMiddleware.install` takes the SDK
   policy snapshot instead of the `&tool_sets` parameter.

## Deletions

- Deleted: `ToolOutputMiddleware`'s `name → Arc<dyn Tool>` policy snapshot in
  `tinyagents/middleware.rs`.
- Remaining by design: `ApprovalSecurityMiddleware`'s args-aware
  `external_effect_with_args` overlay.
- Redundant per-call trait re-queries in `agent_tool_exec.rs` policy chain
  (the chain itself shrinks in `01-tooling/04`).

## Acceptance

- `ToolRegistry::policies()` snapshot test: all tools classified, policies
  serialize stably.
- Approval/security/output behavior parity tests still green
  (middleware suite ~22 tests).
