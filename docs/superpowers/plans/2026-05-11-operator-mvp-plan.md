# Operator MVP - OpenHuman Fork Execution Plan

**Status:** Draft execution plan
**Date:** 2026-05-11
**Primary repo:** `openhuman`
**Reference repo:** `inbox`
**Goal:** Turn the fork into a phone-reachable personal operator that observes work, proposes next actions, asks for approval, acts through trusted connectors, verifies outcomes, and records evidence.

> **For agentic workers:** This is a planning lane artifact. Do not start broad implementation from chat context. Turn selected slices into Linear issues with acceptance criteria, branch names, owned files, validation commands, and PR handoff requirements.

---

## Product Thesis

The product is not a generic desktop assistant.

The product is an operator loop:

```text
Observe -> Propose -> Approve -> Act -> Verify -> Remember
```

Every feature should attach to one of those stages. Anything that does not make that loop faster, safer, or more useful is backlog.

The user-facing promise:

```text
Connect your real work once. The operator finds what needs attention, drafts useful next actions, asks before doing anything risky, executes through the right account/tool, and shows evidence that the work is done.
```

---

## Why This Can Beat The Existing Shape

OpenHuman already has a strong desktop shell, Rust core, controller registry, Composio integration, local memory, skills, screen/voice surfaces, and a growing capability catalog. The current weakness is product focus: many capabilities exist, but the user still has to decide what to ask, where to ask it, and how to know whether the agent actually finished.

The inbox repo already solved the more important product primitive: normalize noisy sources into work to do. It has concrete patterns for:

- local-first source adapters
- normalized thread/item records
- sync state
- source-of-truth routing
- confirmation-gated tools
- narrow MCP tools
- "Now", "Actionable", "Waiting On", and calendar-attached work views

The fork should combine them by making OpenHuman the operator runtime and inbox logic the work-discovery/control plane.

---

## Existing Assets To Reuse

### From OpenHuman

- Tauri desktop app and Rust sidecar runtime.
- Controller registry in `src/core/all.rs`.
- Domain layout under `src/openhuman/<domain>/`.
- Composio controllers in `src/openhuman/composio/`.
- Frontend Composio API wrappers in `app/src/lib/composio/`.
- Skills/runtime surfaces for app actions.
- Memory store and memory tree ingestion/retrieval.
- Capability catalog in `src/openhuman/about_app/`.
- Channels/controllers surface for external channels.
- Voice, screen intelligence, provider surfaces, notifications, and subconscious/background work scaffolding.

### From Inbox

- `message_index_store.py`: operational index with items, threads, sync state, actionability, urgency, open loops, summaries, and sender stats.
- `tools_registry.py`: one registry driving readonly and write-capable tools, with confirmation-gated writes.
- `CONNECTOR_ROADMAP.md`: source adapters -> normalization -> policy -> intent tools.
- `PLAN.md`: organize around work to do, not raw providers.
- `MCP_V1_PLAN.md`: public MCP/chat surface -> private local server, with token gates and confirmation gates.
- Google account/source-of-truth routing rules.
- Gmail, Calendar, Drive, Sheets, Notes, Reminders, GitHub, and iMessage connector experience.

---

## Core Architecture

### New Rust Domain

Create a dedicated domain:

```text
src/openhuman/operator/
|-- mod.rs
|-- types.rs
|-- store.rs
|-- policy.rs
|-- preflight.rs
|-- evidence.rs
|-- goal_packs.rs
|-- inbox_adapter.rs
|-- schemas.rs
|-- ops.rs
`-- ops_tests.rs
```

The domain owns operator state and business logic. React should render operator state and call controllers; it should not decide routing, safety, approval, or source-of-truth behavior.

### Controller Surface

Expose controllers through the existing registry:

```text
openhuman.operator_list_goal_packs
openhuman.operator_create_goal_pack
openhuman.operator_update_goal_pack
openhuman.operator_list_work_items
openhuman.operator_propose_next_actions
openhuman.operator_preview_action
openhuman.operator_create_approval_request
openhuman.operator_approve_request
openhuman.operator_reject_request
openhuman.operator_execute_approved_action
openhuman.operator_record_evidence
openhuman.operator_list_evidence
openhuman.operator_explain_routing_decision
```

Registry additions belong in `src/core/all.rs` and the operator domain schema tests should prove schema/handler parity.

### Data Model

Minimum local tables:

```text
operator_goal_packs
operator_work_items
operator_action_proposals
operator_approval_requests
operator_execution_runs
operator_evidence_events
operator_connector_accounts
operator_policy_rules
operator_sync_state
```

Minimum concepts:

- **GoalPack:** a reusable outcome, trigger, context, permissions, and done criteria.
- **WorkItem:** normalized thing that may need attention.
- **ActionProposal:** draft action the operator wants to take.
- **ApprovalRequest:** explicit user decision required before risk-bearing actions.
- **ExecutionRun:** actual attempt to do approved work.
- **EvidenceEvent:** proof, receipt, diff, message id, PR URL, calendar id, or error trace.
- **PolicyRule:** source-of-truth and permission rule resolved in code, not prompts.

### Goal Pack Shape

```json
{
  "id": "daily-brief",
  "name": "Daily Brief",
  "objective": "Tell me what changed, what needs a reply, and what should be done today.",
  "triggers": [
    { "kind": "schedule", "cron": "0 7 * * MON-FRI" },
    { "kind": "manual", "surface": "phone" }
  ],
  "sources": ["gmail", "calendar", "github", "linear", "imessage"],
  "allowed_actions": ["read", "summarize", "draft", "classify", "propose"],
  "approval_required": [
    "send",
    "create_external_record",
    "update_external_record"
  ],
  "done_criteria": [
    "brief contains top changes",
    "items needing reply are separated from FYI",
    "each proposed action has source links"
  ],
  "evidence_required": true
}
```

---

## Inbox Logic Integration

Do not port the whole inbox repo first. Use an adapter boundary.

### Phase 1 Adapter

OpenHuman calls a local inbox server when present:

```text
OpenHuman operator domain -> inbox_adapter.rs -> http://127.0.0.1:9849
```

Adapter functions:

- `list_actionable_threads`
- `list_waiting_on_me`
- `list_waiting_on_others`
- `list_calendar_context`
- `draft_reply_for_thread`
- `preflight_write`
- `explain_routing_decision`

This gives immediate reuse while keeping OpenHuman stable.

### Phase 2 Native Port

Port only stable primitives into Rust after the product loop is proven:

- normalized `WorkItem` schema
- sync state
- source/account ownership
- actionability classification
- source-of-truth routing
- confirmation gates
- evidence recording

Do not port provider-specific code until a native source has clear user value.

---

## Connector Strategy

### Bootstrap

Use Composio for breadth and speed:

- OAuth handoff
- long-tail connector discovery
- initial Gmail/GitHub/Linear/Slack/Notion-style coverage
- trigger discovery where useful

But do not expose generic Composio execution as the product API. Wrap it in operator policy:

```text
GoalPack -> Proposal -> Preflight -> Approval -> Composio execute -> Verify -> Evidence
```

### Core Connectors

Move high-value connectors to first-class/direct support when they become central to the product:

- Gmail
- Google Calendar
- GitHub
- Linear
- Google Drive/Docs
- Slack
- iMessage/local Messages bridge for power users

### Source-Of-Truth Rule

Routing belongs in code. The model should not guess which account to use.

Examples:

- Replies route through the owning account/thread unless explicitly overridden.
- New Google writes use the configured default account.
- GitHub PR comments route to the repo/owner from the original item.
- Linear updates route to the issue/team from the original work item.

Each write preview must show:

- connector
- account
- target object
- action type
- irreversible risk
- evidence expected after execution

---

## Phone And Chat Surfaces

The near-term goal is to meet users where they already are without pretending iMessage automation is simple or App Store-friendly.

### V0 Surfaces

- Desktop app approval queue.
- Local web approval page.
- Email or push-style notifications for approval requests.
- Telegram/Slack bot as an early phone chat surface.
- iOS Shortcuts + Share Sheet for "send this to operator".
- MCP/chat bridge for ChatGPT or other clients when available.

### Power User Surface

- Local iMessage bridge from macOS using the inbox patterns.
- Treat as a local-only advanced connector, not the primary commercial onboarding path.

### Later Surface

- Messages for Business only if the user base and Apple approval process justify it.
- It is not a general encrypted personal assistant channel by default, and it should not be the MVP dependency.

---

## Data Connect Control Plane

The control plane should store coordination metadata, not raw private data by default.

Candidate hosted tables:

```text
users
devices
connector_accounts
goal_packs
goal_pack_memberships
automation_runs
approval_requests
evidence_events
memory_summaries
channel_threads
sync_cursors
audit_events
```

Rules:

- Raw inbox/email/message content stays local unless the user explicitly opts in.
- Store derived summaries, hashes, source ids, and evidence metadata when enough.
- Every externally visible action records an audit event.
- Cloud unlocks multi-device continuity, phone approval, and backup, not broad data extraction.

TEE/private hosted execution is a later trust upgrade, not a blocker for v0. V0 should be local-first, boring, and auditable.

---

## MVP Goal Packs

### 1. Daily Brief

Outcome:

- show what changed
- show what needs a reply
- show today's meetings and prep
- show blocked/waiting items

Sources:

- Gmail
- Calendar
- GitHub
- Linear
- iMessage if available

Allowed without approval:

- read
- classify
- summarize
- propose

Approval required:

- send
- create issue
- update issue
- archive/delete

### 2. Reply Drafts

Outcome:

- find conversations needing reply
- draft replies in the user's voice
- let user approve, edit, or reject
- send through the owning account only after approval

Evidence:

- source thread id
- sent message id
- account used
- final body hash

### 3. Meeting Prep

Outcome:

- for upcoming meetings, gather calendar details, related threads/docs/issues, and suggested agenda/follow-ups

Evidence:

- calendar event id
- linked source items
- generated prep artifact id

### 4. Engineering Review / Next Goals

Outcome:

- inspect GitHub/Linear state
- identify stale PRs, failed checks, blocked issues, and next execution slices
- propose next Linear issues or PR review tasks

Approval required:

- create/update Linear issues
- comment on GitHub
- push code
- open/close PRs

---

## Safety Policy

### Auto-Allowed

- observe
- classify
- summarize
- draft
- rank
- prepare local previews
- create local evidence records
- run read-only checks

### Approval Required

- send message or email
- create/update/delete external records
- schedule/reschedule meetings
- spend money
- post publicly
- push code
- merge/close PRs
- destructive local filesystem operations
- broad data export

### Always Visible

Every proposal must show:

- what will happen
- why it is being proposed
- what account/source will be used
- what data leaves the device
- what evidence will be recorded
- how to undo or recover when possible

---

## Implementation Gates

### Gate 0 - Fork And Product Boundaries

- [ ] Decide fork name, package identifiers, and user-facing brand copy.
- [ ] Preserve upstream GPL-3.0 obligations for OpenHuman-derived client code.
- [ ] Preserve MIT notices for any inbox-derived code.
- [ ] Decide whether the hosted control plane is separate proprietary service code or open source.
- [ ] Add a `FORK_NOTES.md` or equivalent with upstream sync policy.

Validation:

```bash
git status --short
rg -n "GPL|MIT|license|License" README.md LICENSE* app src docs
```

### Gate 1 - Operator Domain Skeleton

Files:

- `src/openhuman/operator/types.rs`
- `src/openhuman/operator/store.rs`
- `src/openhuman/operator/schemas.rs`
- `src/openhuman/operator/ops.rs`
- `src/openhuman/operator/ops_tests.rs`
- `src/openhuman/operator/mod.rs`
- `src/core/all.rs`

Tasks:

- [ ] Define `GoalPack`, `WorkItem`, `ActionProposal`, `ApprovalRequest`, `ExecutionRun`, and `EvidenceEvent`.
- [ ] Add local SQLite-backed store or reuse the existing app storage pattern.
- [ ] Register list/create/update/list evidence controllers.
- [ ] Add schema parity tests.

Validation:

```bash
cargo test operator --manifest-path app/src-tauri/Cargo.toml
cargo check --manifest-path app/src-tauri/Cargo.toml
```

### Gate 2 - Inbox Adapter

Files:

- `src/openhuman/operator/inbox_adapter.rs`
- `src/openhuman/operator/preflight.rs`
- `src/openhuman/operator/policy.rs`
- `src/openhuman/operator/ops_tests.rs`

Tasks:

- [ ] Add adapter trait for actionable work.
- [ ] Implement local HTTP adapter for inbox server.
- [ ] Add inert/fake adapter for tests.
- [ ] Normalize inbox rows into operator `WorkItem`.
- [ ] Expose `operator_list_work_items` and `operator_explain_routing_decision`.

Validation:

```bash
cargo test operator --manifest-path app/src-tauri/Cargo.toml
```

### Gate 3 - Approval And Evidence Loop

Files:

- `src/openhuman/operator/preflight.rs`
- `src/openhuman/operator/evidence.rs`
- `src/openhuman/operator/ops.rs`
- `src/openhuman/operator/ops_tests.rs`

Tasks:

- [ ] `preview_action` returns account, target, risk, and expected evidence.
- [ ] `create_approval_request` stores a pending approval.
- [ ] `approve_request` transitions pending -> approved.
- [ ] `execute_approved_action` refuses unapproved writes.
- [ ] `record_evidence` links evidence to run/proposal/work item.

Validation:

```bash
cargo test operator:: --manifest-path app/src-tauri/Cargo.toml
```

### Gate 4 - First UI

Files:

- `app/src/lib/operator/`
- `app/src/components/operator/`
- `app/src/pages/Operator.tsx` or the existing app route pattern
- relevant route/nav files

Tasks:

- [ ] Add "Today" view with goal packs and work items.
- [ ] Add approval queue.
- [ ] Add evidence timeline.
- [ ] Add preview drawer for a proposed action.
- [ ] Keep all business logic in Rust controllers.

Validation:

```bash
pnpm --cwd app compile
pnpm --cwd app test -- --run operator
```

### Gate 5 - Composio Execution Wrapper

Files:

- `src/openhuman/operator/connectors.rs`
- `src/openhuman/operator/preflight.rs`
- `src/openhuman/operator/evidence.rs`
- `src/openhuman/composio/` only if narrow changes are required

Tasks:

- [ ] Execute only approved proposals.
- [ ] Map proposal action types to Composio action calls.
- [ ] Record connector result ids as evidence.
- [ ] Refuse generic tool calls from goal packs that lack explicit permission.

Validation:

```bash
cargo test operator composio --manifest-path app/src-tauri/Cargo.toml
```

### Gate 6 - Phone Reachability

Tasks:

- [ ] Add local web approval page or channel controller for approval requests.
- [ ] Add Telegram/Slack bot or MCP/chat bridge as first phone surface.
- [ ] Add iOS Shortcut/share-sheet handoff docs.
- [ ] Keep local iMessage bridge as advanced local connector, not onboarding dependency.

Validation:

```bash
cargo test channels operator --manifest-path app/src-tauri/Cargo.toml
pnpm --cwd app compile
```

---

## First Linear Slices

Each issue should be independently reviewable and should create a real PR.

1. **Operator domain skeleton**
   - Acceptance: schemas compile, registry sees operator controllers, tests prove schema/handler parity.
   - Validation: `cargo test operator --manifest-path app/src-tauri/Cargo.toml`.

2. **Goal pack store**
   - Acceptance: create/list/update goal packs round trip through local store.
   - Validation: `cargo test operator::goal_packs --manifest-path app/src-tauri/Cargo.toml`.

3. **Approval request state machine**
   - Acceptance: pending/approved/rejected/executed transitions are enforced.
   - Validation: `cargo test operator::approval --manifest-path app/src-tauri/Cargo.toml`.

4. **Evidence event store**
   - Acceptance: evidence links to work item/proposal/run and is queryable.
   - Validation: `cargo test operator::evidence --manifest-path app/src-tauri/Cargo.toml`.

5. **Inbox adapter trait plus fake adapter**
   - Acceptance: operator can list normalized work items without real inbox server.
   - Validation: `cargo test operator::inbox_adapter --manifest-path app/src-tauri/Cargo.toml`.

6. **Inbox HTTP adapter**
   - Acceptance: when inbox server is reachable, actionable threads map to `WorkItem`; when unreachable, UI/API returns a clear degraded state.
   - Validation: adapter unit tests plus one manual local smoke.

7. **Preflight/routing explanation**
   - Acceptance: previews show source, account, target, risk, and expected evidence.
   - Validation: `cargo test operator::preflight --manifest-path app/src-tauri/Cargo.toml`.

8. **Operator Today UI**
   - Acceptance: user can see goal packs, actionable items, approvals, and evidence.
   - Validation: `pnpm --cwd app compile` and operator component tests.

9. **Composio approved execution wrapper**
   - Acceptance: unapproved writes fail; approved writes call the connector wrapper and record evidence.
   - Validation: Rust tests with fake connector.

10. **Daily Brief goal pack**
    - Acceptance: daily brief can run in preview mode and produce evidence-backed proposed actions.
    - Validation: unit tests plus a local seeded-data smoke.

---

## Proof Metrics

MVP is useful only if these are true:

- Time from install to first useful proposed action is under 5 minutes.
- A non-technical user can connect at least one account and understand what the operator wants to do.
- Every write action has an approval record.
- Every completed action has evidence.
- The operator can explain why it chose an account/source.
- The user can reject a proposal without breaking the workflow.
- The product is useful with one connector and gets better with more connectors.

---

## Non-Goals For V0

- Full mobile app.
- Messages for Business as primary onboarding.
- Hosted TEE execution.
- Open-source model hosting as a product dependency.
- Marketplace for goal packs.
- Generic all-connector automation.
- Porting the whole inbox repo into Rust.
- Autonomous writes without approval.

---

## Decision Rules

- If a feature does not produce a work item, proposal, approval, action, or evidence event, it is probably not MVP.
- If a model is deciding account routing from prose, move the rule into code.
- If a connector action cannot be previewed, it cannot be executed.
- If an action cannot produce evidence, it should not claim completion.
- If a phone surface cannot carry approvals clearly, it is only a notification surface.
- If a goal pack cannot define done criteria, it is too vague.
