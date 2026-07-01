# 07.3 — Steering surface + one recursion authority

## Steering

1. Map product tools onto crate commands: `steer_subagent` →
   `SteeringCommand::InjectMessage`/`Redirect` via `SteeringRegistry`;
   pause/resume/cancel → corresponding variants; keep delivery-at-safe-
   boundaries semantics (crate drains before each model call).
2. Install a `SteeringPolicy` allowlist per run (e.g. background runs
   accept Cancel only).
3. Accepted/rejected steering emits `AgentEvent::Steered` → bridge → UI;
   delete bespoke acknowledgment plumbing in `run_queue/`.
4. `harness/run_queue/` (345): collapse to a constructor of
   `SteeringHandle`s; delete the queue types the crate replaces.

## Recursion

5. One cap: crate `RunLimits.max_depth`/`RecursionPolicy` becomes
   authoritative; `spawn_depth_context.rs` (66) becomes a reader/projector
   of crate depth (`ToolExecutionContext.depth`) for product error wording,
   or is deleted if the wording can wrap `TinyAgentsError::SubAgentDepth`.
6. One error shape: map `SubAgentDepth`/`RecursionLimit` to the existing
   JSON-RPC error for compat.

## Deletions

- `harness/run_queue/` queue mechanics.
- `harness/spawn_depth_context.rs` (if step 5 wording-wrap suffices).

## Acceptance

- Mid-flight steer lands only at loop boundaries (existing steering tests);
  depth-exceeded produces the same user-facing error as today.
