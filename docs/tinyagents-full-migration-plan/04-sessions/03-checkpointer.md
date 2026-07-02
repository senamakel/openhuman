# 04.3 — SqliteCheckpointer swap

Requires 00-baseline (rusqlite 0.40 + `sqlite` feature).

Current status (2026-07-02): do not delete
`src/openhuman/tinyagents/checkpoint.rs` yet. `SqlRunLedgerCheckpointer` is still
the live durable checkpointer for delegation graphs, and it writes OpenHuman's
`graph_checkpoints` run-ledger schema. The crate `SqliteCheckpointer` uses its
own checkpoint schema, so swapping it in requires an explicit row migration or
documented expiry policy for in-flight durable graph runs.

## Steps

1. Point durable graphs (delegation, workflow scheduler, teams — currently
   on `SqlRunLedgerCheckpointer` or `FileCheckpointer`) at crate
   `SqliteCheckpointer` only after the schema migration/expiry decision is
   implemented.
2. Migration for existing `graph_checkpoints` rows in the session DB:
   either a one-time copy into the crate schema or documented expiry
   (checkpoints are resume-state; expiring in-flight runs at upgrade is
   acceptable ONLY if no long-lived durable runs exist — audit
   `workflow_runs` retention first).
3. Keep `CheckpointMetadata` (thread_id/checkpoint_id/parent/namespace)
   projected into the run ledger for the command-center listing RPC —
   a read-side projection, not a second writer.
4. Standardize `DurabilityMode` per graph (Sync for approval-interrupt
   graphs, Async for fanout).

## Deletions

- Later: `src/openhuman/tinyagents/checkpoint.rs`
  (`SqlRunLedgerCheckpointer`, 251) + the `graph_checkpoints` table creation
  once migration/expiry ships. Retain it while delegation still points at the
  OpenHuman run-ledger schema.

## Acceptance

- Durable graph interrupt → process restart → resume from exact checkpoint
  (e2e per graph).
- Command center still lists checkpoints/current node.
