# tinyflows — Completion & Integration Plan (POA)

> **Status**: proposed · **Date**: 2026-07-04
> **Scope**: finish integrating the vendored `vendor/tinyflows/` workflow engine (our n8n/Zapier module) into the Rust core and the desktop UI, and close the feature gaps that separate it from a real n8n-class product.
> **Reference product**: [n8n](https://github.com/n8n-io/n8n) — item-based data flow, `=`-expressions, node catalog, editable canvas, credentials, webhook/schedule triggers, executions view.

---

## 1. Where we are today

The integration is **not greenfield**. Three layers already exist; each is at a different maturity level.

### 1.1 Engine (`vendor/tinyflows/`, v0.3.0 — mostly done)

Host-agnostic library crate on the `tinyagents` state-graph runtime. Pipeline: `WorkflowGraph → migrate → validate → compile → engine::run`.

| Area | State |
| --- | --- |
| Node kinds (12) | ✅ `trigger, agent, tool_call, http_request, code, condition, switch, merge, split_out, transform, output_parser, sub_workflow` |
| Routing | ✅ linear, conditional (ports), parallel fan-out, merge fan-in barrier |
| Data flow | ✅ n8n-style item arrays `{ json, binary?, paired_item? }`; `=`-expressions with **full jq** (`jaq-core`) already wired in `src/expr.rs` |
| Error handling | ✅ per-node `on_error` (stop/continue/route), retry with fixed/exponential backoff, `node_timeout_secs` |
| HITL | ✅ `requires_approval` → interrupt; in-process (`run_resumable`) and cross-process durable resume (`resume_with_checkpointer(thread_id)`) |
| Observability | ✅ `RunObserver` trait (`on_run_start/on_step_finish/on_run_finish`), tracing spans, journaled variants (`GraphEventJournal`) for Langfuse |
| Persistence | By design **none** — host injects `Checkpointer`, `StateStore`, and persists `Run`/`ExecutionStep` |
| Capabilities | ✅ host-injected traits: `LlmProvider`, `ToolInvoker`, `HttpClient`, `CodeRunner`, `StateStore`; opaque `connection_ref` (host resolves secrets) |
| Versioning | ✅ `schema_version` (graph) + per-node `type_version`, `migrate()` pre-parse |
| Triggers | ⚠️ **declarative only** — `manual, schedule, webhook, app_event, form, execute_by_workflow, chat_message, evaluation, system`; the *host* must fire them |

Engine-side gaps (see Phase 6): `agent` node sub-ports (chat_model/memory/tool/output_parser) stubbed; `output_parser` is identity passthrough; README/Roadmap lag the code (claim jq and retry backoff are pending when both ship).

### 1.2 Rust core seam (`src/openhuman/flows/` + `src/openhuman/tinyflows/` — implemented, with holes)

- **Domain** `flows::` (~3,700 lines + tests): `types.rs` (`Flow` wraps `WorkflowGraph` + `enabled`/`require_approval`/`last_status`; `FlowRun`, `FlowRunStep`, `FlowRunTrigger::{Rpc,Schedule,AppEvent,Resume}`), `store.rs` (SQLite incl. `flow_state` kv), `ops.rs` (validate/migrate + full run/resume under `TrustedAutomation → Workflow` origin, 600 s timeout), `schemas.rs`, `tools.rs` (`ProposeWorkflowTool` — validate-only, never persists), `bus.rs` (`FlowTriggerSubscriber`).
- **RPC surface** (10 methods, wired in `src/core/all.rs`): `openhuman.flows_{create,get,list,update,delete,set_enabled,run,resume,list_runs,get_run}`.
- **Capability seam** `src/openhuman/tinyflows/`: `caps.rs` (LLM/Composio-tools/HTTP/code/state adapters), `observability.rs` (currently `NoopObserver`), `langfuse_export.rs` (post-run trace export).
- **Schedule triggers work end-to-end**: `flows::ops::bind_schedule_trigger` registers a cron `JobType::Flow` → scheduler publishes `DomainEvent::FlowScheduleTick` → `FlowTriggerSubscriber` runs the flow.

**Known core gaps** (each becomes a workstream below):

| # | Gap | Evidence |
| --- | --- | --- |
| G1 | **Webhook triggers unwired** — enabling a webhook-trigger flow only logs a warning; `bus.rs` observes `WebhookIncomingRequest` but explicitly does not dispatch | `flows/ops.rs:298` (`log_webhook_trigger_deferred`), `flows/bus.rs:232-242` |
| G2 | **No live run observer** — `NoopObserver`; `FlowRunStep`s reconstructed post-hoc from final state, no per-step timing/attempts, nothing streamed while running | `flows/types.rs:76`, `flows/ops.rs:781-783` (`TODO(0.3)`) |
| G3 | **Credential / connected-account resolution stubbed** — Composio nodes fall back to the ambient signed-in account; toolkit allow-listing hard-rejects real toolkits; HTTP credential resolution unimplemented (`connection_ref` is accepted but unresolvable) | `tinyflows/caps.rs:193,235-261,330,376,408` |
| G4 | **No cancel/deny** — a dismissed approval leaves the run parked `pending_approval` forever; no `flows_cancel`/`flows_deny` RPC | UI comment in `FlowApprovalCard.tsx` |
| G5 | **No JSON-RPC E2E coverage** — zero `openhuman.flows_*` calls in `tests/json_rpc_e2e.rs` (unit tests only) | grep of `tests/*.rs` |
| G6 | Unfired trigger kinds: `chat_message`, `form`, `execute_by_workflow` (as a *trigger*), `evaluation`, `system` have no host dispatcher | `flows/bus.rs` |

### 1.3 Frontend (`app/src/` — reachable, read-only)

Shipped: `/flows` nav tab (FlowsPage list: enable toggle, Run, last status), `/flows/:id` **read-only** canvas (`@xyflow/react` v12, custom nodes, minimap), `FlowRunsDrawer` + `FlowRunInspectorDrawer` (2 s polling via `useFlowRunPoller`), chat `WorkflowProposalCard` (agent `propose_workflow` → user "Save & enable" is the **only** creation path), `FlowApprovalCard` HITL notifications. All components have co-located Vitest coverage; i18n namespaces `flows.*`/`flowRuns.*`/`notifications.flow.*` exist across locales.

**Known UI gaps**:

| # | Gap | Evidence |
| --- | --- | --- |
| U1 | **No canvas editing** — `nodesDraggable/Connectable/elementsSelectable: false`; `xyflowToWorkflowGraph` in `graphAdapter.ts` is dead code awaiting the editor ("B5b.2+") | `components/flows/canvas/FlowCanvas.tsx` |
| U2 | **No node config panel** — clicking a node does nothing; `config` shown only as truncated hint chips | — |
| U3 | **No authoring entry** — "New workflow" navigates to `/chat` with a TODO; empty-state copy promises canvas creation that doesn't exist | `pages/FlowsPage.tsx` |
| U4 | No trigger-config UI, no credentials picker, no template gallery, no import/export | — |
| U5 | Approval "Dismiss" is client-only (blocked on G4) | `FlowApprovalCard.tsx` |
| U6 | Run progress is poll-only; no socket push (blocked on G2) | `hooks/useFlowRunPoller.ts` |

### 1.4 Disambiguation (do not conflate)

The repo has **three** "workflow" systems. This plan touches only the first:

1. `flows::` / `openhuman.flows_*` — **tinyflows typed graphs** (this plan).
2. `workflows::` / `openhuman.workflows_*` — WORKFLOW.md/SKILL.md bundle discovery/install (separate product surface under `/skills`).
3. `rlm::` — Rhai `.ragsh` language workflows (`docs/plans/rlm-workflows/`), positioned in `gitbooks/features/orchestration.md` as the *next* layer on the same substrate. tinyflows remains the shipping visual/typed product; rlm does not replace it.

---

## 2. Gap analysis vs n8n / Zapier

What an n8n user would expect, mapped to our state:

| n8n capability | tinyflows/OpenHuman today | Covered by |
| --- | --- | --- |
| Editable node canvas (drag, connect, add/delete) | Read-only viewer | Phase 3 |
| Node config side-panel with schema-driven forms | Missing | Phase 3 |
| Webhook trigger with live URL | Declared but never fired | Phase 1 |
| Cron/interval trigger | ✅ shipped | — |
| App-event trigger (≈ Zapier polling/instant triggers) | Wired via Composio `ComposioTriggerReceived`, but account resolution stubbed | Phase 1 (G3) |
| Credentials store + per-node credential picker | `connection_ref` plumbing exists; resolution unimplemented; no UI | Phase 2 |
| Executions list + live run view | List + inspector exist; polling, post-hoc steps only | Phase 1–2 |
| Cancel a running/paused execution | Missing | Phase 1 |
| Expressions (`={{ }}` in n8n) | ✅ `=`-prefix + full jq | — |
| Error workflow / retry per node | ✅ on_error/retry/backoff | — |
| Sub-workflows | ✅ `sub_workflow` node (inline graph); *by-id* reference missing | Phase 4 |
| Templates gallery | Missing | Phase 4 |
| Import/export workflow JSON, n8n import | Missing (feasible: model is deliberately n8n-shaped) | Phase 4 |
| Partial execution / pin data / step-through debug | Missing | Phase 5 (stretch) |
| Waiting/Wait node, human approval | ✅ HITL approval + durable resume | — |
| AI Agent node with attached tools/memory | `agent` node = bare LLM call; sub-ports stubbed | Phase 6 |
| Versioning of workflow definitions | ✅ schema_version + type_version + migrate | — |
| Multi-user sharing/RBAC | Out of scope (desktop, single-user + team domain later) | — |

Our structural advantages to preserve: the agent can *author* workflows conversationally (`propose_workflow` → proposal card → single human "Save & enable" persistence gate), everything runs under the security policy/approval-gate substrate, and secrets never enter the engine (opaque `connection_ref`).

---

## 3. Plan of action

Ordering rationale: **backend correctness first** (triggers, cancellation, live observation, credentials) because every UI phase consumes those RPCs; the editable canvas is the largest UI lift and is independent of trigger work, so it proceeds in parallel from Phase 3.

### Phase 1 — Backend completion (triggers, lifecycle, observability)

**1a. Webhook triggers (G1)** — the flagship Zapier-style gap.
- On `flows_create`/`flows_update`/`set_enabled(true)` with a `webhook` trigger: provision an inbound route via `webhooks::ops` (incl. `create_tunnel` when remote reachability is needed), store `{ flow_id → webhook_route }` in the flow record, tear down on disable/delete.
- In `flows/bus.rs`: implement dispatch on `DomainEvent::WebhookIncomingRequest` — match route → enabled flow, seed trigger payload `{ method, headers (allow-listed), query, body }`, run under `FlowRunTrigger::Webhook` (new variant).
- Response semantics v1: fire-and-forget `202` with `run_id` (n8n "respond immediately"); "respond with last node output" deferred (needs request/response bridging — note as follow-up).
- Security: webhook-triggered runs are `TrustedAutomation → Workflow` origin like schedule ticks; payload is untrusted input — route through `prompt_injection` screening before any `agent` node consumes it.
- RPC additions: `flows_get` returns `webhook_url` when applicable.

**1b. Run lifecycle: cancel + deny (G4).**
- New RPCs `openhuman.flows_cancel_run(run_id)` (terminal `cancelled` status; abort the tokio task / drop the checkpointed thread) and deny semantics on resume: `flows_resume(id, thread_id, approvals, rejections)` → rejected node routes to its `error` port or fails the run.
- Sweep: TTL for parked `pending_approval` runs (align with the 10-min approval-gate TTL, configurable per flow).

**1c. Live run observation (G2).**
- Implement a real `RunObserver` in `src/openhuman/tinyflows/observability.rs`: `on_step_finish` → persist `FlowRunStep` incrementally (timing, attempt count, status, output) via `flows::store`, and publish a new `DomainEvent::FlowRunProgress { run_id, node_id, status }`.
- Bridge to the frontend socket (`socket` domain) so the inspector can subscribe instead of polling (UI lands in Phase 2/3; keep polling as fallback).
- Wire the journaled run variants so Langfuse export happens per-step, not only post-run.

**1d. Remaining trigger kinds (G6).**
- `chat_message`: dispatch from the channels/thread pipeline (opt-in per flow: channel filter in trigger config).
- `execute_by_workflow`: `sub_workflow`-by-id + a `flows_run` call path from another flow (see 4b).
- `form`, `evaluation`, `system`: explicitly document as *not yet fired*; validation should warn when a flow's trigger kind has no live dispatcher instead of silently never running.

**1e. E2E tests (G5).**
- Extend `tests/json_rpc_e2e.rs` (+ `scripts/test-rust-with-mock.sh`): full lifecycle round-trip (create → run → steps → resume → cancel → delete), schedule-tick dispatch, webhook dispatch, approval park/deny. This is the merge gate for 1a–1d.

**Deliverable**: every trigger kind either fires or loudly warns; runs can be cancelled/denied; run steps stream.

### Phase 2 — Credentials & connections (G3)

The `connection_ref` seam exists end-to-end but resolves nothing. This phase makes integration nodes real.

- **Composio connected accounts**: resolve `connection_ref` → specific Composio connected account (today: ambient-account fallback). Fix toolkit allow-listing in `caps.rs:193` so real toolkits pass policy instead of being hard-rejected. Surface account choice in the node config (`connection_ref` = connected-account id).
- **HTTP credentials**: back `connection_ref` for `http_request` nodes with the existing `credentials` domain (header/bearer/basic templates stored encrypted; injected server-side in `OpenHumanHttp::request` — never returned to the UI or the engine).
- **RPCs**: `flows_list_connections()` (aggregate Composio connected accounts + stored HTTP credentials, ids + display names only) for the UI picker.
- **Policy**: `code` and `http_request` node execution must respect the `[autonomy]` tier / `classify_command`-equivalent gating (network class); document the matrix in `flows/ops.rs`.

**Deliverable**: an `http_request` or Composio `tool_call` node can be pointed at a chosen account/credential by reference, with secrets never leaving the core.

### Phase 3 — Editable canvas (U1–U2) — the n8n builder

Largest UI phase; runs in parallel with Phase 2 once Phase 1c's RPCs exist.

- **3a. Edit mode in `FlowCanvas`**: flip the readonly defaults behind an `editable` prop; enable drag (persist `position`), connect (port-aware: derive valid source/target handles from node kind — reuse `graphAdapter` port logic), delete nodes/edges, and a node palette (12 kinds with the existing emoji/accent metadata). Wire the already-written `xyflowToWorkflowGraph` as the save path → `flows_update`.
- **3b. Node config panel**: right-hand drawer on node select. v1 pragmatic approach: per-kind form components for the high-traffic kinds (`trigger` schedule/webhook config, `http_request` method/url/headers/body, `agent` prompt/model, `tool_call` slug/args, `condition`/`switch` expression, `transform` set-map, `code` editor with language toggle) + a raw-JSON escape hatch for the rest. `=`-expression fields get a monospace input with an "expression" affordance (full expression-editor with live preview is a stretch goal — needs a `flows_eval_expr` RPC against sample run data).
- **3c. Validation UX**: new RPC `openhuman.flows_validate(graph)` (thin wrapper over `ops::validate_and_migrate_graph`, same path `propose_workflow` uses) → inline canvas errors (missing trigger, cycle, invalid config on node X) before save.
- **3d. Draft/dirty state**: local draft in component state; explicit Save; unsaved-changes guard. No autosave in v1 (a saved+enabled flow is live — accidental saves fire schedules).
- **3e. Live run overlay**: subscribe to `FlowRunProgress` socket events; animate node status on the canvas during a run (n8n's signature interaction) and in the inspector.

**Deliverable**: create/edit a workflow entirely on the canvas; watch it execute live.

### Phase 4 — Authoring entry points, templates, interop (U3–U4)

- **4a. "New workflow"**: replace the `/chat` TODO with a chooser — *Start from scratch* (blank canvas with a trigger node), *Describe it* (prefill chat composer → `propose_workflow` path), *From template*.
- **4b. Sub-workflow by id**: extend `sub_workflow`/`execute_by_workflow` to reference a saved `flow_id` (engine currently only inlines a child graph) — engine change upstreamed to `vendor/tinyflows` + host resolver in `caps`/ops.
- **4c. Templates**: ship 5–10 curated `WorkflowGraph` JSONs (bundled resources, like agent prompts): e.g. "Daily digest to channel", "Webhook → agent triage → notify", "Scheduled scrape → transform → memory". Gallery UI on FlowsPage empty state.
- **4d. Import/export**: export flow JSON; import with `migrate()` + validate. **n8n importer** (host-side, best-effort): map n8n workflow JSON → `WorkflowGraph` for the overlapping vocabulary (IF→condition, Switch, Merge, SplitOut, HTTP Request, Code, Schedule/Webhook triggers; `={{...}}` → `=` jq where trivially translatable); unmapped nodes land as annotated placeholder nodes rather than failing the import.
- **4e. Proposal card upgrade**: "Open in canvas" action on `WorkflowProposalCard` (review/edit before Save & enable) — keeps the single persistence gate.

### Phase 5 — Polish & debug tooling (stretch)

- Partial execution ("run from this node" with pinned upstream data) — needs engine support for seeding node state; scope with tinyflows maintainers.
- Run diff/inspector niceties: per-item data browser (n8n's table/JSON toggle), input↔output pairing via `paired_item`.
- `flows_duplicate`, run retention/pruning policy, per-flow run-history limits.
- Desktop E2E (WDIO) spec: create → run → inspect happy path.

### Phase 6 — Engine (vendor/tinyflows) upstream work

Tracked separately since it's a submodule with its own release cadence (host pins `0.3`, patched to path):

1. `agent` node sub-ports (chat_model/memory/tool/output_parser wiring) — unlocks n8n-style "AI Agent with tools" composition.
2. `output_parser`: schema validation + LLM auto-fix (currently identity).
3. Sub-workflow by reference (4b) — config `workflow_id` alternative to inline `workflow`.
4. Docs truth-up: README/Roadmap still claim jq and retry backoff are pending; both are implemented (`src/expr.rs` routes to jaq; `engine.rs` has fixed/exponential backoff + `node_timeout_secs`).
5. Optional: cancellation token support in `engine::run` (cleaner than task-abort for 1b).

---

## 4. Missing-features summary (checklist)

Backend: ☐ webhook trigger provisioning+dispatch · ☐ `flows_cancel_run` · ☐ resume-with-rejection/deny · ☐ live `RunObserver` + incremental step persistence · ☐ `FlowRunProgress` socket events · ☐ `flows_validate` RPC · ☐ `flows_list_connections` · ☐ Composio connected-account resolution · ☐ HTTP credential resolution · ☐ toolkit allow-list fix · ☐ `chat_message` trigger dispatch · ☐ sub-workflow by id · ☐ parked-run TTL sweep · ☐ JSON-RPC E2E suite.

Frontend: ☐ editable canvas (drag/connect/palette/delete) · ☐ node config panels · ☐ trigger config UI (cron builder, webhook URL display, app-event picker) · ☐ credentials picker · ☐ new-workflow chooser · ☐ template gallery · ☐ import/export + n8n import · ☐ live canvas run overlay (socket) · ☐ approval deny (real) · ☐ "Open in canvas" from proposal card · ☐ WDIO E2E spec.

Engine (upstream): ☐ agent sub-ports · ☐ output_parser validation · ☐ `workflow_id` sub-workflows · ☐ docs truth-up · ☐ cancellation.

---

## 5. Sequencing, estimates, risks

```
Phase 1 (backend)      ██████        ~2–3 wk   unblocks everything
Phase 2 (credentials)     ████       ~1–2 wk   parallel w/ Phase 3
Phase 3 (canvas)          ████████   ~3–4 wk   largest UI lift
Phase 4 (authoring)               ████  ~2 wk
Phase 5 (polish)                    ██  opportunistic
Phase 6 (engine)       ── continuous, PR'd to vendor submodule ──
```

**Risks / decisions to confirm:**
1. **Webhook exposure model** — desktop app behind NAT: v1 relies on `webhooks::ops::create_tunnel` (backend dependency); local-LAN-only webhooks as fallback. Needs product sign-off on URL lifetime/auth (per-flow secret token in the path).
2. **Prompt-injection surface** — webhook/app_event payloads feeding `agent` nodes are untrusted; must go through the `prompt_injection` domain. Non-negotiable before G1 ships.
3. **Editable canvas vs. agent-first authoring** — product stance so far is "the agent builds it, you approve it" (`gitbooks/features/workflows.md`). Phase 3 makes hand-editing first-class; keep the proposal-card path primary in onboarding copy so the two don't compete.
4. **GPL-3.0 licensing** of `vendor/tinyflows` — already vendored/linked; re-confirm distribution posture before expanding surface (flag for legal, not an engineering blocker).
5. **Submodule cadence** — engine changes (Phase 6) must land in the tinyflows repo and re-vendor; keep host-side workarounds (e.g. by-id sub-workflow resolution in caps) so UI phases aren't blocked on vendor releases.

**Per-repo conventions that apply to all phases**: new RPCs go through domain `schemas.rs` + controller registry (no `dispatch.rs` branches); ≥80 % diff coverage gate; verbose `[flows]`-prefixed debug logging on every new path; i18n keys added to `en.ts` **and all 13 locales**; update `src/openhuman/about_app/` and `gitbooks/features/workflows.md` as features land.
