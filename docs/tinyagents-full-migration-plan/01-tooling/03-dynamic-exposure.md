# 01.3 — Dynamic tool exposure as middleware

Replace scattered pre-filtering with crate selection middleware.

Current OpenHuman surfaces: `agent/harness/tool_filter.rs` (mechanics),
`tools/user_filter.rs`, sub-agent `tool_prep.rs`, per-turn assembly in
`session/turn/tools.rs`, channel permission ceiling checks.

## Steps

1. Express agent `tool_allowlist`/`tool_denylist`, sub-agent tool scope,
   MCP tool visibility, and the channel permission ceiling as a composed
   `ToolAllowlistMiddleware` + one OpenHuman
   `ContextualToolSelectionMiddleware` (`ToolSelectionContext` carries agent
   id, task kind, tier, channel). Inheritance rule: children can only narrow.
2. Fail closed when policy metadata is missing (unclassified → not exposed);
   emit exposure decisions through events (`RouteSelected`/observation
   metadata) for audit.
3. Move the visible-set computation out of `session/turn/tools.rs` (693
   lines) and `subagent_runner/tool_prep.rs` (350 lines) into the middleware;
   the turn code only declares candidate tool sets.
4. Keep product policy sources (registry definitions, security tier tables)
   in OpenHuman — the middleware consumes them.

## Deletions

- `agent/harness/tool_filter.rs` mechanics (keep policy tables if any inline).
- `subagent_runner/tool_prep.rs`.
- Filtering blocks in `session/turn/tools.rs` (file shrinks to assembly glue).

## Acceptance

- Sub-agents can never see a tool the parent couldn't grant (test).
- Exposure decision visible in run events.
- CLI/RPC-only denial (`CliRpcOnlyMiddleware`) unaffected or folded in.
