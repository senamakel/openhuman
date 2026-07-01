# 00 — Baseline: crate, features, native links

## Steps

1. **Bump `tinyagents` to latest 1.2.x** (root `Cargo.toml` pins `"1.2"`;
   `cargo update -p tinyagents` in both Cargo worlds — root and
   `app/src-tauri/`). Known 1.1→1.2 break already handled
   (`MessageDelta::text` ctor).
2. **Align rusqlite to 0.40** in both worlds (`Cargo.toml:130` root,
   `app/src-tauri/Cargo.toml:145`, currently `0.37` bundled). tinyagents'
   `sqlite` feature pins `rusqlite 0.40` bundled; the `links = "sqlite3"`
   conflict is the only blocker. Fix openhuman call sites for the 0.37→0.40
   API delta, then enable `tinyagents = { version = "1.2", features =
   ["sqlite"] }`.
3. **Unlocks:** crate `SqliteCheckpointer` → later deletion of
   `src/openhuman/tinyagents/checkpoint.rs` (`SqlRunLedgerCheckpointer`,
   251 lines) once graphs are re-pointed and `graph_checkpoints` rows are
   migrated or expired (see `04-sessions/`).
4. **Do NOT enable `openai` feature** — OpenHuman providers stay the product
   source of truth for credentials/billing (spec non-goal).
5. Update the "TinyAgents crate: features & compatibility" section in
   `gitbooks/developing/architecture/agent-harness.md` and the Cargo.toml
   comment (lines 46–55) after the flip.
6. Mark `docs/tinyagents-sdk-gaps.md` items 1, 2, 3, 4, 7, 10, 11, 12
   as shipped-in-1.2.0 (verified against local crate source at
   `~/.cargo/registry/src/*/tinyagents-1.2.0/`); keep only the residuals:
   no free-form `ToolSchema` metadata map, no reasoning field on
   `AgentEvent::ModelDelta` payload, no `root_run_id` on `RunConfig`.

## Acceptance

- Both Cargo worlds `cargo check` clean with `sqlite` feature on.
- One duplicate-free `cargo tree -i libsqlite3-sys` per world.
- Docs updated; sdk-gaps marked.
