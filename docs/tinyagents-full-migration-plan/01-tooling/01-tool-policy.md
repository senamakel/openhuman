# 01.1 — ToolPolicy round-trip

The spec's "SDK gap: ToolSchema has no metadata map" is obsolete: the crate
`Tool` trait has `policy() -> ToolPolicy` and `ToolRegistry::policies()`.

## Steps

1. In `src/openhuman/tinyagents/tools.rs` (`ToolAdapter`/`SharedToolAdapter`),
   implement `policy()` by mapping OpenHuman trait methods
   (`src/openhuman/tools/traits.rs`): permission level → `ToolAccess
   { approval_required, background_safe }`; `external_effect` →
   `ToolSideEffects`; timeout policy → `ToolRuntime.timeout_ms`;
   concurrency safety → `ToolRuntime.idempotent`; `max_result_size_chars`
   → `ToolRuntime.max_result_bytes`; sandbox mode → `SandboxMode`.
   Every adapter-produced policy sets `classified: true`.
2. Rework `ApprovalSecurityMiddleware` and `ToolOutputMiddleware`
   (`src/openhuman/tinyagents/middleware.rs`) to read `ToolPolicy` from the
   registry instead of the shared `name → Arc<dyn Tool>` side-lookup.
   Keep args-aware checks (`external_effect_with_args`) as an OpenHuman
   overlay where the static policy is insufficient — document each overlay.
3. Install crate `ToolPolicyMiddleware` in `assemble_turn_harness`
   (`src/openhuman/tinyagents/mod.rs`) fail-closed; assert every registered
   tool is classified in the adapter-inventory test.
4. Delete the side-lookup plumbing from `TurnContextMiddleware.install`
   (the `&tool_sets` parameter) once no middleware needs it.

## Deletions

- `name → Arc<dyn Tool>` side-lookup in `tinyagents/middleware.rs`.
- Redundant per-call trait re-queries in `agent_tool_exec.rs` policy chain
  (the chain itself shrinks in `01-tooling/04`).

## Acceptance

- `ToolRegistry::policies()` snapshot test: all tools classified, policies
  serialize stably.
- Approval/security/output behavior parity tests still green
  (middleware suite ~22 tests).
