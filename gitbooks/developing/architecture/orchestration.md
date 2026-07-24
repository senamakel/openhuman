# Subconscious orchestration layer

The orchestration layer is OpenHuman's device-side client for the hosted
orchestration brain. Wrapped Claude Code / Codex sessions talk to the owner
agent over tiny.place Signal-encrypted DMs. OpenHuman decrypts and caches those
messages, forwards events and world diffs to the backend, renders hosted
sessions, and executes explicitly requested local effects.

Domain root: [`src/openhuman/orchestration/`](../../../src/openhuman/orchestration).
Design spec: [`docs/arch-subconscious.md`](../../../docs/arch-subconscious.md) and
the staged plan under [`docs/plans/subconscious-orchestration/`](../../../docs/plans/subconscious-orchestration).

## End-to-end flow

```text
Claude Code / Codex session
  └─ tinyplace harness wrapper — tails the session JSONL → SessionEnvelopeV1
       └─ Signal E2E DM → owner agent's tiny.place inbox   [tagged sessionId, source, role]
            └─ ingest (decrypt-once → classify → cache → hosted event → ack)
                 └─ hosted brain runs the orchestration cycle
                      └─ pushed effects → local effect executor
            └─ world-diff uploader → hosted subconscious tier
  UI: Brain → Orchestration tab (orchestration.* RPC + orchestration:message socket)
```

## Hosted boundary

The reasoning/wake graph is owned by `tinyhumansai/backend`. OpenHuman owns:

- E2E DM decrypt, classification, dedupe, local render caching, and ack order;
- `POST /orchestration/v1/events` and world-diff uploads;
- hosted session/message read synchronization and reachability;
- pushed-effect validation and execution; and
- direct Medulla request/response tool loops.

The retired `GET /orchestration/v1/steering` route is not part of this boundary.
OpenHuman neither polls nor caches a current steering directive.

## Memory subconscious

The device memory subconscious remains a local
`observe → prepare_context → reflect → commit` pipeline. Observation, context
scouting, scheduling, checkpointing, and tools stay on-device. With
`subconscious.engine = "auto"` (the default), changed windows use
`POST /orchestration/v1/run` with `flavor: "openhuman"` and the bounded
subconscious tool catalogue. Preflight unavailability can fall back to the local
decision agent. Once the run request is submitted, failures do not fall back,
which prevents duplicate notifications or state writes. `engine = "local"`
bypasses hosted reflection.

## RPC + UI (stage 7)

Renderer-only controllers (internal registry) in `orchestration/schemas.rs`:
`openhuman.orchestration_{sessions_list, messages_list, send_master_message,
mark_read, status}`. Status reports reachability and tick/ingest health, not a
steering directive. Live updates ride an `orchestration:message` socket event
(`bus.rs` broadcast → `core/socketio.rs` bridge) fanned out for every persisted
chat message. The Brain → Orchestration tab (`TinyPlaceOrchestrationTab.tsx` +
`useOrchestrationChats.ts`) reads real store classification, live-updates, and lets
the owner steer the front-end agent from the Master composer.

The `openhuman.orchestration_run` renderer RPC is the direct request/response
entry point for the backend's paid Medulla engine. It performs a paid-plan
preflight, advertises OpenHuman's local contact, session-history, and
send-to-agent tools, and executes requested calls on the device. Tool results
are returned through `/orchestration/v1/run/continue` until Medulla produces a
final reply. The backend repeats authentication, entitlement, and resource
limit enforcement on every cycle.

## Running unattended

- **No message loss**: ingest dedupes by relay `message_id` *before* decrypt (the
  Signal ratchet is never advanced twice); a relay/decrypt error leaves the message
  un-acked for a clean retry.
- **Malformed input**: any non-envelope / malformed DM body falls back to the peer's
  Master window; the parser never panics.
- **No duplicate reflection actions**: local fallback is allowed only for typed
  preflight failures. Submitted-cycle failures hold the memory baseline.
- **Observability**: logs carry counts, phase, and correlation ids without
  message bodies, decrypted plaintext, tool arguments, or backend response
  bodies.

## Configuration

The `[orchestration]` config block (`src/openhuman/config/schema/orchestration.rs`)
uses `enabled` as its master switch. Optional direct-cycle overrides are grouped
under `[orchestration.medulla.prompt_overrides]`,
`[orchestration.medulla.config]`, and `[orchestration.medulla.limits]`. Omitted
values retain backend defaults, and the backend clamps all supplied graph and
resource limits.
