# C2b — Task board / todos onto `graph::todos` (parity note)

Status: **first slice landed** (branch `feat/tinyagents-c2-todos`). Adapter-first,
shadow-only. Legacy stays authoritative; nothing here changes product behavior.

This note pairs with the CONTINUATION plan §C2 (step 3) and records what the
crate `tinyagents::graph::todos` surface maps onto in OpenHuman, and what it does
**not** — the residue that must stay in the product host after a future cutover.

## What landed in this slice

- `src/openhuman/todos/graph_shadow.rs` — the adapter:
  - Total, lossless status mapping OpenHuman ↔ crate (`map_status_to_crate` /
    `map_status_from_crate`) — the two `TaskCardStatus` enums share the same
    seven variants (`Todo`, `AwaitingApproval`, `Ready`, `InProgress`,
    `Blocked`, `Done`, `Rejected`).
  - `TaskBoardCard` field-by-field conversion (`to_crate_card`) — all metadata
    (objective, plan, assignedAgent, allowedTools, approvalMode,
    acceptanceCriteria, evidence, notes, blocker, sessionThreadId,
    sourceMetadata, order, updatedAt) preserved.
  - `spawn_mirror` — after every authoritative `todos::ops::save_cards` write of
    a `Thread` board, mirrors the persisted cards into a crate `FileStore`
    (`<workspace>/tinyagents_graph_store`) under namespace `graph.todos`. Fire-
    and-forget, log-only; a crate rejection (e.g. the single-`InProgress`
    invariant) is warn-logged as a **DIVERGENCE**.
  - `spawn_shadow_claim` — wired into `todos::ops::claim_card` (which is the
    single claim entry-point the dispatcher's two claim sites in
    `task_dispatcher/dispatch.rs` funnel through, plus RPC/reclaim callers). It
    seeds the crate board with the pre-claim snapshot and replays the crate
    `claim_card` CAS, warn-logging when the crate ok/err verdict disagrees with
    the authoritative legacy claim.

The legacy claim/save path is byte-for-byte preserved: `claim_card` was
refactored to compute one ok/err verdict via an extracted `apply_claim` helper
(so the shadow sees the same not-found / wrong-status / invariant outcomes), but
the persisted result and all existing tests are unchanged.

## Crate `TodoTool` vs `agent/tools/todo.rs` — what maps

The crate ships a single multiplexer `TodoTool` (`op`-dispatched) that is a near
drop-in for the OpenHuman `todo` tool. Shared surface:

| Concern | Crate `TodoTool` | OpenHuman `tools/todo.rs` | Match? |
| --- | --- | --- | --- |
| Dispatch style | single tool, `op` field | single tool, `op` field | yes |
| Thread binding | `ToolExecutionContext::thread_id` (never an arg) | `thread_context::current_thread_id()` + `fork_context` parent | yes (both bind to current thread, never an arg) |
| `add`/`edit`/`update_status`/`remove`/`replace`/`clear`/`list` | present | present | yes |
| Optional card fields | objective/plan/assignedAgent/allowedTools/approvalMode/acceptanceCriteria/evidence/notes/blocker | same | yes |
| Return shape | `{ threadId, cards, markdown }` | `{ threadId, cards, markdown }` | yes |
| Status aliases | `parse_status` (pending→todo, approved→ready, …) | `ops::parse_status` (identical alias table) | yes |
| Single-`InProgress` invariant | `enforce_single_in_progress` (hard error) | `enforce_single_in_progress` (identical) | yes |
| `claim_card` CAS | `store::claim_card(expected, target)` | `ops::claim_card(expected, target)` | yes (identical semantics; proven by shadow tests) |

## What does NOT map (product residue — must stay in the host)

1. **Approval-gate coupling.** OpenHuman's `todo` tool stamps a default
   `approvalMode` by reading `config.autonomy.require_task_plan_approval`
   (`default_task_approval_mode`), and the dispatcher's
   `requires_plan_approval` + `TaskPlanAwaitingApproval` `DomainEvent` drive the
   interactive plan-review gate. The crate `TodoTool` has **no** config read and
   **no** approval-gate wiring — it exposes `decide_plan`/`revise_plan` state
   transitions only. The gate policy stays product.
2. **`DomainEvent` emissions.** `ops::claim_card`/mutations emit
   `AgentProgress::TaskBoardUpdated` (via `fork_context` `on_progress`) and the
   dispatcher publishes `TaskPlanAwaitingApproval`. The crate store emits
   nothing. All event vocabulary stays product (ledger: keep).
3. **RPC projection shapes.** `threads.task_board_*` and `openhuman.todos_*`
   (see `todos/schemas.rs`) are the wire contracts the kanban UI binds to
   (`app/src/services/api/todosApi.ts`, `USER_TASKS_THREAD_ID = "user-tasks"`).
   These are **unchanged** by this slice and, per §C2, become read-side
   projections over the crate store only at cutover — not now.
4. **Scratch board.** OpenHuman has a thread-less in-memory `BoardLocation::Scratch`
   fallback (tool calls outside a chat thread). The crate board is always
   `(Store, thread_id)`, so scratch mutations have **no** crate mirror target —
   the shadow skips them (trace-logged).
5. **Persistence substrate + timestamps.** Product persists RFC3339
   `updated_at` to `<workspace>/agent_task_boards/<hex(thread_id)>.json`; the
   crate uses epoch-millis strings in a `Store` namespace. The mirror does not
   reconcile timestamps (cosmetic). Card-id minting also differs
   (`task-<uuid>` product vs `task-<seq>` crate) but ids are passed through, so a
   persisted board round-trips.
6. **Run lifecycle.** `todos/runs.rs` (run records, heartbeats, stale-reclaim)
   and `task_dispatcher/` executor mechanics (executor resolution, autonomous
   run, board write-back) are **not** part of the crate todos surface — they
   stay product and are the §C2 step-3 "runner node" work, tracked separately.

## Single-writer constraint

The crate `Store` has no compare-and-set, so ns `graph.todos` assumes a single
writer. The core process is that single writer (both the mirror and the
shadow-claim run in-core). Documented in the module header; honoured because all
mutations funnel through `todos::ops`.

## Next (not in this slice)

- Flip the mirror from shadow to authoritative (crate store becomes the source
  of truth; legacy JSON becomes a projection or is retired).
- Reimplement `threads.task_board_*` / `openhuman.todos_*` as projections over
  the crate store.
- Replace the dispatcher claim/poll loop with the crate `claim_card` CAS + a
  graph runner node, keeping `DomainEvent` emission + channel bindings product.
- Delete `task_board.rs` + todo CRUD mechanics + dispatcher executor mechanics
  (~3.2k + tests) once parity logs are clean.
