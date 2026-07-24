# Hosted Medulla Subconscious

## Summary

OpenHuman still polls `GET /orchestration/v1/steering`, although the backend
removed that route and its underlying steering model during the Medulla v2
migration. The stale poll runs on every hosted orchestration sync pass and
produces a recurring 404.

This change removes the retired steering contract from OpenHuman and makes the
backend's current Medulla request/response API the preferred reflection engine
for the memory subconscious. OpenHuman continues to own observation,
checkpointing, scheduling, local tool execution, and safety policy. The existing
local decision agent remains available as a safe fallback.

## Goals

- Stop all requests to the retired steering endpoint.
- Remove steering-only state and controls that can no longer represent live
  backend behavior.
- Run changed memory windows through hosted Medulla by default for eligible
  signed-in users.
- Preserve the existing subconscious action surface and approval behavior.
- Preserve local subconscious behavior when hosted execution is known to be
  unavailable before a cycle starts.
- Advance the memory baseline only after a successful reflection path.
- Prevent a local fallback from duplicating a hosted cycle that may already have
  been accepted.

## Non-goals

- Restore the backend's retired steering model, directive history, or
  `runSubconsciousTick` implementation.
- Reintroduce the TinyPlace steering profile.
- Replace the local observation, heartbeat, trigger, checkpoint, or status
  systems.
- Move memory-source data or credentials into backend persistence.
- Adopt the interactive `/medulla/v1/sessions` API for subconscious ticks.
- Change ordinary TinyPlace orchestration sessions or their historical message
  rendering.

## Current-state diagnosis

Backend commit `037793ec` removed the orchestration steering model, synthesis
loop, cycle injection, controller, and HTTP route as part of the Medulla v2
inversion. Medulla now steers each visit in its orchestrator rather than
maintaining a separate offline directive.

OpenHuman still:

- defines `/orchestration/v1/steering` in `orchestration/cloud.rs`;
- fetches it on each 20-second read-sync pass;
- caches the result under `orch:steering`;
- returns that cache from `orchestration.status`; and
- renders the value and a manual review control in the TinyPlace orchestration
  UI.

The backend's supported Medulla surface is now `/orchestration/v1/run` plus
`/orchestration/v1/run/continue`. OpenHuman already has a tested client for this
tool loop in `orchestration/medulla.rs`.

## Architecture

### 1. Retired steering cleanup

The hosted orchestration reader will continue to synchronize sessions and
messages and to maintain cloud reachability. It will no longer fetch steering.

Remove:

- `STEERING_PATH` and `ReadPass::fetch_steering`;
- `STEERING_KEY`, steering cache writes, and `cached_steering`;
- the steering member of `OrchestrationStatus` and its schema assembly;
- steering props, header, badges that depend on a current directive, and the
  "Run review" action from the orchestration UI; and
- tests and translations that exist only for those removed controls.

Pinned and ordinary orchestration chats remain available so historical messages
are not discarded merely because the directive status surface was retired.

### 2. Hosted reflection adapter

The memory subconscious remains an OpenHuman-owned
`observe -> prepare_context -> reflect -> commit` pipeline:

1. `observe` computes the local memory diff.
2. Quiet windows commit locally without an inference request.
3. `prepare_context` keeps using the local read-only context scout.
4. `reflect` selects hosted Medulla or the local decision agent.
5. `commit` advances the local memory checkpoint only after reflection succeeds.

The hosted path calls `/orchestration/v1/run` with:

- `flavor: "openhuman"`;
- the rendered memory diff and prepared context as the input;
- the existing subconscious decision guidance;
- a bounded, explicit tool catalogue; and
- the existing Medulla tuning limits where applicable.

The client drives `/run/continue` until it receives a terminal `end` event.
Pending events are polled and tool-use events execute on the device before their
results are returned to the backend.

The existing orchestration Medulla client will be refactored just enough to
accept a caller-supplied flavor, tool set, and workload context. The public
`orchestration.run` behavior remains unchanged.

### 3. Tool and safety boundary

Hosted Medulla receives only the action surface currently granted to the memory
subconscious:

- `notify_user`
- `update_task`
- `goals_list`
- `goals_add`
- `goals_edit`
- `spawn_subagent`

The tools execute inside OpenHuman. The hosted model never receives filesystem
or credential access through this adapter.

The complete hosted tool loop runs under the same
`AgentTurnOrigin::TrustedAutomation` origin used by the local subconscious
agent. External-content observations use the tainted source. Internal
observations use the untainted source. Tool policy and the approval gate
therefore continue to decide whether a requested action may run.

Sub-agent delegation receives a root parent context and remains blocking for
this headless workflow. Its result is returned within the Medulla tool loop
rather than sent to a nonexistent chat delivery thread.

### 4. Selection and fallback

Hosted Medulla is the preferred engine. The local decision agent is selected
when unavailability is known before a hosted cycle can start, including:

- no live app-session token;
- hosted orchestration disabled;
- inactive or ineligible plan;
- a failed read-only eligibility preflight;
- unsupported `openhuman` flavor; or
- missing device registration rejected before cycle creation.

Fallback is not permitted after submitting the run request if the backend may
have accepted it. A transport failure, continuation failure, timeout, or
terminal hosted error after that boundary fails the reflection. The local
baseline remains unchanged so the failure is visible and recoverable, but the
same tick is not immediately executed locally.

This distinction requires typed hosted errors that identify whether the failure
occurred before or after the submission boundary. String matching is not an
acceptable fallback decision mechanism.

`subconscious.engine` becomes a two-mode setting:

- `auto` (default): prefer hosted Medulla and use the safe local fallback rules
  above;
- `local`: always use the current local decision agent.

The old draft value `medulla` deserializes as `auto` for configuration
compatibility. Its former meaning (a local `medulla-serve` child) is retired
from the subconscious pipeline. The separate `medulla_local` diagnostic/RPC
domain is outside this change and remains available to developers, but it no
longer drives subconscious ticks.

### 5. State and observability

No backend session identifier is persisted for subconscious ticks because the
request/response tool-loop surface is used. Local state remains authoritative
for:

- the memory baseline checkpoint;
- last successful tick time;
- total tick and failure counters;
- rate-cap/provider status; and
- graph checkpoint cleanup on the local fallback path.

Logs identify the selected engine and classify fallback without logging memory
diff contents, tool arguments, user-authored text, or backend error bodies that
may contain sensitive data.

The status surface continues to report the memory subconscious instance. This
change does not add a persisted "last engine" field.

## Error handling

- Quiet observation: commit locally; no hosted request and no fallback.
- Hosted eligibility unavailable: run the local reflection once.
- Hosted terminal success: commit locally.
- Local fallback success: commit locally.
- Hosted failure after submission: record a failed tick and do not commit.
- Local fallback failure: preserve the existing failed-tick behavior and do not
  commit.
- Tool-level failure: return the structured failure to Medulla so it can reason
  about the result; only a terminal successful cycle permits commit.
- Superseded tick: preserve the generation check and skip commit even if the
  hosted call completed.

## Testing

### Orchestration cleanup

- A read sync requests sessions and messages but never requests steering.
- Status serialization contains no steering field.
- The TinyPlace orchestration UI has no steering header or manual review action.
- Existing session/message rendering and reachability behavior remain covered.

### Hosted Medulla adapter

- The initial request includes `flavor: "openhuman"` and only the allowed tool
  specifications.
- A direct terminal response commits the observation.
- Tool-use events execute the matching local tool and send the result through
  `/run/continue`.
- Pending events poll to completion.
- Unknown or failed tools return structured errors.
- Trusted-automation origin and taint reach tool execution.
- Blocking sub-agent execution receives a valid root parent context.

### Fallback and commit safety

- Signed-out, unpaid, disabled, and preflight-unreachable states select the
  local path.
- A known pre-acceptance flavor/device rejection selects the local path.
- A failure after run submission never invokes the local path.
- Failed or superseded hosted runs do not advance the baseline or
  `last_tick_at`.
- Successful hosted and local runs each advance the baseline exactly once.

## Delivery sequence

1. Remove the retired steering poll/cache/status/UI contract and commit it as an
   independently validated fix.
2. Refactor the Medulla client to support a caller-supplied flavor and tool set,
   preserving the existing orchestration RPC, and commit.
3. Add the hosted subconscious adapter and fallback selector with focused tests,
   then commit.
4. Remove the superseded local `medulla-serve` branch from subconscious,
   migrate the engine configuration to `auto | local`, and update
   documentation, then commit.
5. Run targeted Rust and frontend suites, formatting, type checking, and the
   disabled-feature checks affected by any feature-gate edits.
