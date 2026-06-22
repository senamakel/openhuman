# Make subagents asynchronous and reusable by default

## Summary

Subagent delegation should default to a durable, asynchronous worker model. When the orchestrator delegates work to a specialist, it should prefer reusing an existing compatible subagent across turns, preserving that subagent's cached prefix, worker thread, steering queue, and accumulated task context. The harness should spawn a fresh subagent only when the requested agent type, toolkit, permissions, worktree, or task identity is materially different.

This is a GitHub issue draft based on the current harness implementation.

## Problem

OpenHuman already has the pieces for long-running subagent work, but the default orchestration contract still treats most delegation as a one-shot spawn:

- `spawn_subagent` runs a typed subagent inline and returns one compact final result to the parent.
- `spawn_async_subagent` can fire-and-forget work, while `steer_subagent` and `wait_subagent` can communicate with that running task.
- `create_worker_thread` persists subagent conversations under a parent thread, so the UI can reopen the parent-to-subagent transcript.
- `run_subagent` supports checkpoint replay through `initial_history`, but only for explicit paused clarification flows.
- Harness docs already call out KV-cache stability as important for parent sessions and fork context, but durable cache reuse is not the default subagent lifecycle.

The result is that an orchestrator handling a multi-turn task can easily spawn multiple short-lived researchers, coders, or integration workers for what is logically the same job. That loses useful subagent-local context, increases prefill/cost, fragments worker-thread history, and makes follow-up instructions depend on the parent remembering and manually passing enough context.

The desired behavior is closer to a reusable specialist roster:

- The orchestrator can say "continue the research worker with this new constraint" on the next user turn.
- The harness can route that message into the same durable child when the work identity still matches.
- The child keeps its useful transcript/cache and does not need a full prompt-and-context rebuild.
- A new child is created only when the job is meaningfully different or incompatible with the existing worker.

## Current state researched

- Main harness sessions preserve a byte-stable system prompt across turns for provider prefix cache reuse: `gitbooks/developing/architecture/agent-harness.md`.
- Parent-to-subagent context is passed through `ParentExecutionContext`, including provider, model, tool registry, memory context, connected integrations, progress sink, and workspace lineage.
- `spawn_subagent` currently creates a new `task_id` and usually creates a worker thread, then invokes `run_subagent` inline with `run_queue: None`.
- `spawn_async_subagent` creates a `RunQueue`, registers the running child in `running_subagents`, and returns a task reference that can be steered or waited on.
- `running_subagents` is keyed by transient `task_id` and parent session ownership. Terminal entries are pruned; it is not a durable worker identity index.
- `continue_subagent` resumes from checkpoint files only after `ask_user_clarification`; it is not a general "continue this specialist across turns" mechanism.
- `SubagentRunOptions.initial_history` proves the runner can resume an existing child history, but no reusable default lifecycle is wired around it.
- The docs still describe `spawn_subagent` as "spin up a fresh agent with its own context window", which conflicts with the desired default.

## Proposed solution

Introduce a first-class durable subagent session model and make delegation route through it by default.

### 1. Add durable subagent identity

Create a stable `subagent_session_id` separate from transient `task_id`.

The identity should include:

- parent conversation/thread id
- agent id/archetype
- toolkit or skill scope, when present
- model override, when present
- sandbox mode and permission-relevant execution shape
- worktree/action-root identity for code workers
- normalized task title or explicit task key

The session should point at the worker thread and current runnable state. A task may have many `task_id` runs over time, but one durable `subagent_session_id`.

### 2. Add a reuse policy before spawning

Before creating a child, the orchestrator-facing delegation tool should ask the registry for a compatible reusable session.

Reuse when:

- the agent id and toolkit/scope match
- the parent thread matches
- the requested task key/title is the same or semantically close enough under deterministic heuristics
- permissions, sandbox, model, and action root are compatible
- the child is running, paused, idle, or recently completed with a reusable transcript

Spawn fresh when:

- the requested agent type changes
- the toolkit/integration scope changes
- the worktree or action root changes
- the prior child is terminal and marked not reusable
- the new work is a clearly unrelated job
- the user or orchestrator explicitly asks for a fresh worker

Start with deterministic matching. Do not add another model call to classify reuse in the first implementation.

### 3. Make async the default behavior

Change the delegation contract so the default path behaves like an asynchronous reusable worker:

- if the compatible child is running, inject the message through its queue
- if it is idle/completed but reusable, resume it with prior history plus the new instruction
- if it is paused awaiting user input, route the user's answer or follow-up into the paused session
- if no compatible worker exists, create one and register it as durable

Inline blocking should become an explicit mode, not the default. The orchestrator should be able to answer the user with a short "started/continued worker" response and collect results later through idle completions, `wait_subagent`, or a worker drawer.

### 4. Persist reusable history and cache hints

Persist enough child state to resume after process restart:

- worker thread id
- agent id and display name
- parent thread id
- toolkit/scope/model/sandbox/worktree metadata
- latest run status
- compacted child history or transcript pointer
- cache-affinity metadata for provider/model selection
- created/updated/last-used timestamps

Keep the existing byte-stable prompt discipline: dynamic per-turn additions should enter as user-visible context/messages rather than rewriting the subagent's system prompt.

### 5. Expose explicit orchestrator tools

Replace the current "spawn or manually remember task ids" surface with clearer operations:

- `delegate_subagent` or updated `spawn_subagent`: default reusable async delegation.
- `message_subagent`: send a steer/collect/follow-up to a durable child by `subagent_session_id` or compatible selector.
- `wait_subagent`: accept durable `subagent_session_id` as well as transient `task_id`.
- `list_subagents`: show active/reusable children for the current parent thread.
- `close_subagent`: explicitly end a reusable child when it should not receive future work.

These tools should still enforce parent-session/thread ownership and existing allowlists.

## Acceptance criteria

- [ ] **Durable identity** - the harness has a persisted `subagent_session_id` model linked to parent thread, worker thread, agent id, scope metadata, status, and last-used timestamps.
- [ ] **Default reuse** - orchestrator delegation reuses a compatible existing subagent across parent turns before spawning a new one.
- [ ] **Default async** - the default delegation path returns quickly and registers a steerable/waitable worker; blocking inline delegation is an explicit option.
- [ ] **Cross-turn messaging** - a later user turn can route a new instruction to the same reusable subagent without requiring the parent model to remember a transient `task_id`.
- [ ] **Resume from idle/completed** - an idle reusable subagent can restart from persisted history using `SubagentRunOptions.initial_history` or an equivalent transcript replay path.
- [ ] **Running-child steering** - a running reusable subagent receives new messages through `RunQueue` without restarting.
- [ ] **Spawn-new guardrails** - materially different agent type, toolkit, permission/sandbox shape, worktree/action root, or explicit fresh-task request creates a new subagent.
- [ ] **UI visibility** - the worker/subagent drawer can display reusable children, current status, and whether a worker was reused or newly spawned.
- [ ] **Observability** - logs use stable prefixes such as `[subagent_reuse]`, include correlation fields (`parent_thread_id`, `subagent_session_id`, `task_id`, `agent_id`, `reuse_decision`), and avoid secrets or full PII.
- [ ] **Tests** - Rust tests cover reuse matching, spawn-new incompatibility, running-child steering, idle-child resume, restart persistence, and ownership rejection.
- [ ] **Docs** - update `gitbooks/developing/architecture/agent-harness.md`, `gitbooks/features/native-tools/agent-coordination.md`, and `docs/DELEGATION_POLICY.md` so they no longer describe subagents as fresh-by-default.
- [ ] **Diff coverage >= 80%** - the implementing PR meets the changed-lines coverage gate.

## Non-goals

- Do not let worker-tier subagents recursively spawn arbitrary workers.
- Do not reuse across unrelated parent threads by default.
- Do not use semantic/model-based reuse classification in the first pass.
- Do not expose other users' subagent sessions or bypass existing approval/sandbox policy.
- Do not log prompts, credentials, raw integration payloads, or full worker transcripts as correlation metadata.

## Implementation notes

- The durable registry likely belongs near `src/openhuman/agent_orchestration/running_subagents.rs`, but should not be purely in-memory. The in-memory map can remain the live-task index; the durable store should survive process restarts.
- `worker_thread.rs` already centralizes parent-linked worker-thread creation and is a good place to keep the conversation-thread link stable.
- `spawn_async_subagent` already has the closest live lifecycle semantics: `RunQueue`, background task registration, completion recording, and idle delivery.
- `continue_subagent` and checkpoint replay show how to resume a child history, but the new model should generalize this beyond clarification-only resumes.
- The parent-visible issue is mostly an orchestration/API contract change; the core runner can stay shared through `run_subagent`.

## Related files

- `src/openhuman/agent_orchestration/tools/spawn_subagent.rs`
- `src/openhuman/agent_orchestration/tools/spawn_async_subagent.rs`
- `src/openhuman/agent_orchestration/tools/steer_subagent.rs`
- `src/openhuman/agent_orchestration/tools/wait_subagent.rs`
- `src/openhuman/agent_orchestration/tools/continue_subagent.rs`
- `src/openhuman/agent_orchestration/running_subagents.rs`
- `src/openhuman/agent_orchestration/tools/worker_thread.rs`
- `src/openhuman/agent/harness/subagent_runner/`
- `src/openhuman/agent/harness/run_queue/`
- `gitbooks/developing/architecture/agent-harness.md`
- `gitbooks/features/native-tools/agent-coordination.md`
- `docs/DELEGATION_POLICY.md`
