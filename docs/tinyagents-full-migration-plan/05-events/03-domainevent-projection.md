# 05.3 — DomainEvent & AgentProgress as projections

`DomainEvent` (core event bus) and `AgentProgress` (UI vocabulary) stay —
as compatibility projections fed from crate events, never as primary state.

## Steps

1. Catalog current agent-domain `DomainEvent` publishers/subscribers
   (`agent/bus.rs`, triage, task_dispatcher, orchestration
   background_delivery/run_ledger_finalize). The run-ledger finalizer subscriber
   is now crate-internal registration glue. For each event that mirrors a
   crate `AgentEvent`/`GraphEvent`, source it from the journal/bridge.
   Events with no crate analogue (triage decisions, channel routing) stay
   native DomainEvents — they are product semantics.
2. `agent/bus.rs` native handler `agent.run_turn` stays (transport into the
   turn), but its progress mirroring moves to the bridge.
3. Document the rule in `core/event_bus` README: new agent-run telemetry
   reads crate events/status first; DomainEvent is for cross-domain product
   signals.
4. Do NOT remove `DomainEvent` variants until every subscriber is re-pointed
   (spec non-goal) — track per-variant status in this file as work lands.

## Deletions

- Duplicate mirroring blocks identified in step 1 (list them here when
  cataloged).

## Acceptance

- Existing subscribers receive identical DomainEvents (bus tests).
- New run-inspection RPCs read journals/status, not the bus.
