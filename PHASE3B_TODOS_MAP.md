# Phase 3b — todos → tinyagents graph::todos crate cutover (FULL ASYNC conversion)

Precedent: Phase 3a did the same for thread_goals (commit 3050c0a64) — see
`src/openhuman/thread_goals/{store.rs,crate_adapter.rs}` + the boot-migration
`spawn_thread_goals_migration` in `src/core/runtime/services.rs`. thread_goals
was already async; todos is NOT — hence the async cascade below.

## Authoritative store
`src/openhuman/agent/task_board.rs` (single file, ~605 lines). `TaskBoardStore::{new,get(:137),put(:191),delete(:253)}` + free fns `board_for_thread(:305)`, `normalise_board(:313)`. Persists file-JSON at `<workspace>/agent_task_boards/<hex(thread_id)>.json`. All SYNC (std::fs). Card id `task-<uuid>` (normalise_board :326). Timestamp RFC3339 (`Utc::now().to_rfc3339()`).

## Crate: `tinyagents::graph::todos` (vendor/tinyagents/src/graph/todos/)
Types field-for-field identical (TaskBoardCard/TaskBoard/TaskCardStatus/TaskApprovalMode/CardPatch/TodosSnapshot). Store (async, `store: &Arc<dyn Store>`, ns `graph.todos`, key `hex(thread_id)`): list/add/edit/update_status/set_session_thread/decide_plan/revise_plan/remove/replace/clear/claim_card(CAS). `enforce_single_in_progress` rejects >1 InProgress. `render_markdown` byte-identical to `todos/ops.rs::render_markdown`. Card id `task-<seq>`, timestamp epoch-millis (both opaque strings — leave existing ids/stamps as-is; keep minting `task-<uuid>` locally before handing to crate `replace`).

## Mapping (TaskBoardStore/ops → crate)
get→store::list(.cards); put(board)→store::replace(store,tid,cards); delete→store::clear + existence pre-check to keep the `bool`; ops add/edit/update_status/remove/replace/clear/list/decide_plan/revise_plan/set_session_thread → same-named crate fns; ops::claim_card→store::claim_card. Scratch (`BoardLocation::Scratch`) has NO crate target → keep on the in-memory `ScratchTodoStore` (`todos/store.rs`) local path; only `BoardLocation::Thread` routes to crate. Per-thread key byte-identical (hex(thread_id)).

## THE BLOCKER: sync→async
TaskBoardStore::{get,put,delete} + todos::ops::* are SYNC; crate store is ASYNC. Chosen fix: make them async, add `.await` at all sites. (Do NOT block_on — panics in a tokio worker.)

## Reusable conversion helpers (in `todos/graph_shadow.rs`)
`map_status_to_crate`/`_from_crate`, `map_approval_mode`, `to_crate_card` (has it), `thread_target`. MISSING: `from_crate_card` (crate→Oh) — ADD it for the read path. `crate_store_for` currently points at shadow dir `tinyagents_graph_store` — REPOINT to the shared tree `open_session_stores(workspace).kv` (like `thread_goals::crate_adapter::crate_goals_store` at crate_adapter.rs:40), ns `graph.todos`. Rename module to `todos/crate_adapter.rs` to match goals.

## Consumer census (all sites to `.await`-convert when signatures go async)
Type-only consumers (INSULATED, no change): agent/task_session.rs; agent/task_dispatcher/{tests,dispatch,prompt,poller,executor}.rs; agent/tools/todo.rs, update_task_tests.rs; agent/progress.rs:256; agent/triage/escalation.rs:332,589,616; task_sources/route.rs:292; threads/turn_state/{types,mirror_tests}.rs; skills/e2e_run_tests.rs:32.
Direct TaskBoardStore callers (async .await needed): threads/tools.rs:726 (+board_for_thread :21); threads/schemas.rs:759 (+:714); todos/ops.rs:136,161.
todos::ops::* callers (async .await needed): agent/task_dispatcher/{poller:12,executor:19,dispatch:6,7,types:3,tests:6}; agent/tools/{todo:12, update_task:15,123 fn apply}; agent/triage/{escalation:316,333,570,573,590,617, envelope:13,22,200,242}; task_sources/route.rs:17-18,28-29,294 + store_tests:270,285; skills/e2e_run_tests:43-44.
todos::runs::* (STAYS LOCAL — no crate equiv): agent/task_dispatcher/{executor:20,poller:13,dispatch:7}. runs.rs persists `<workspace>/agent_task_boards/<hex>.runs.json` — keep as-is.
Scratch/store: agent/tools/todo.rs:310,321 global_scratch_store; BoardLocation::Scratch at todo.rs:209,221 + task_dispatcher/tests.rs:26,59.
RPC reg: src/core/all.rs:662-666 (all_todos_registered_controllers, ns `todos`). Tools glob: tools/mod.rs:63. Frontend: app/src/services/api/todosApi.ts (id constant mirror — unaffected).

## Ordered edits
1. Repoint store handle to shared tree + ns graph.todos; add from_crate_card; rename graph_shadow.rs→crate_adapter.rs keeping conversion helpers.
2. Re-back TaskBoardStore::{get,put,delete} + ops load_cards(:130)/save_cards(:146) onto crate list/replace/clear via adapter (TinyAgentsError→String). Make them ASYNC; cascade `.await` to ops::* then all callers (groups above). `update_task.rs:123 fn apply` becomes async.
3. Boot migration: `migrate_legacy_task_boards_into_crate_store(workspace)` mirroring crate_adapter.rs migrate_legacy_goals (read `<workspace>/agent_task_boards/<hex>.json`, skip `*.runs.json`, idempotent `replace` into graph.todos). Wire behind a `static Once` beside `spawn_thread_goals_migration` in core/runtime/services.rs.
4. Drop shadow scaffold: spawn_mirror/spawn_shadow_claim/spawn_best_effort + call sites ops.rs:166,581. Keep conversion helpers.
5. Optional: delete local duplicate render_markdown/normalise_board/parse_status/enforce_single_in_progress in favour of crate.

## Risks
async/sync (block_on panic — go async); single-InProgress invariant (crate replace ENFORCES, put did not — any raw invalid-board save now errors); claim CAS concurrency model parking_lot→tokio async mutex (rewrite `concurrent_claims_only_one_wins` ops.rs:1083 for async); delete(bool) vs clear(empty) — existence pre-check; markdown parity (keep golden test); runs.rs stays local; AgentProgress::TaskBoardUpdated + emit_progress stay in local ops around crate calls.

## Tests to update
task_board.rs inline tests (:412-604); todos/ops.rs inline (:689-1133, esp concurrent_claims :1083 + scratch :1121); todos/invariant_tests.rs; graph_shadow.rs tests (→adapter tests or delete); consumer tests (task_dispatcher/tests, update_task_tests, task_sources/store_tests, skills/e2e_run_tests, threads/turn_state/mirror_tests) need .await; add boot-migration test.
