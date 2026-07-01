# 04.3 — SqliteCheckpointer swap

Requires 00-baseline (rusqlite 0.40 + `sqlite` feature).

## Steps

1. Point durable graphs (delegation, workflow scheduler, teams — currently
   on `SqlRunLedgerCheckpointer` or `FileCheckpointer`) at crate
   `SqliteCheckpointer`.
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

- `src/openhuman/tinyagents/checkpoint.rs` (`SqlRunLedgerCheckpointer`, 251)
  + the `graph_checkpoints` table creation once migration/expiry ships.

## Acceptance

- Durable graph interrupt → process restart → resume from exact checkpoint
  (e2e per graph).
- Command center still lists checkpoints/current node.
