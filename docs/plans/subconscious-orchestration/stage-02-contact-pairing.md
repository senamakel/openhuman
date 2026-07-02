# Stage 2 — Contact pairing & DM authorization (OpenHuman ⇄ session identities)

## Goal command

> Build the **authentication/pairing flow** between the user's OpenHuman tiny.place identity and
> each wrapped Claude Code / Codex session identity. The relay **refuses 1:1 DMs between
> non-contacts** (`PUT /messages` → `403 not_a_contact`; see
> `backend-tinyplace-v2/docs/spec/contacts.md`), so before any stage-1 envelope can flow, the two
> identities must hold an **accepted mutual contact edge**. Expose the contacts API through the
> core `tinyplace` domain, implement a user-consented pairing policy in the orchestration domain,
> and surface link/approve UX in the Brain tab. Contact requests carry no free text by design
> (prompt-injection-safe bootstrap) — the pairing signal is the edge itself, never message content.

## Backend facts this stage builds on

- Contact model (`tiny.place/sdk/typescript/src/types/contacts.ts`, backend `docs/spec/contacts.md`):
  one edge per unordered pair, `pending | accepted | blocked`, `requester`/`addressee` direction.
- Routes: `POST /contacts/{agentId}` (request), `POST …/accept`, `POST …/block`, `…/unblock`,
  `DELETE /contacts/{agentId}`, `GET /contacts`, `GET /contacts/requests`,
  `GET /contacts/{agentId}/status`, `GET /contacts/stats`. All signed; reads too (graph is private).
- **Crossing requests auto-accept**: if A→B is pending and B sends B→A, the edge converges to
  `accepted` — this is the mechanism that makes user-initiated linking frictionless.
- Send is idempotent for duplicate outgoing / already-accepted; refused when blocked.

## Read first

- `backend-tinyplace-v2` — `docs/spec/contacts.md`, `docs/spec/messaging.md` (DM gate),
  `internal/controllers/relay/controller.go` (enforcement).
- `tiny.place/sdk/typescript/src/api/contacts.ts` + `types/contacts.ts` — client surface to mirror.
- `src/openhuman/tinyplace/{mod.rs, schemas.rs, ops.rs, state.rs}` — controller pattern; note
  there is **no contacts support in the core domain today** (net-new).
- `app/src/lib/agentworld/invokeApiClient.ts` — no `contacts` namespace yet (net-new).
- Approval surface precedent: `src/openhuman/approval/` + `ApprovalRequestCard` (frontend), and
  `DomainEvent::ProactiveMessageRequested` for notification-style delivery.
- Stage 1 (`stage-01-tinyplace-session-bridge.md`) — the wrapper-side half of the handshake.

## Pairing flows (both must work)

**A. User-initiated link (recommended, zero unsolicited approvals):**
1. Wrapper prints its identity (`agentId` + optional `@handle`) at startup (stage 1).
2. User pastes it into "Link a session" in the Brain tab → core sends `POST /contacts/{sessionId}`.
3. Wrapper, which is already polling `GET /contacts/{owner}/status`, sees `pending incoming` from
   its configured `--tinyplace-forward-to` identity and accepts (it only ever auto-accepts from
   that exact configured identity) — or, if the wrapper requested first, the owner's request
   crosses and **auto-accepts** server-side. Envelopes start flowing.

**B. Session-initiated request (approval-gated):**
1. Wrapper sends the contact request to the owner identity and queues envelopes (stage 1).
2. Core polls/ingests `GET /contacts/requests` → each new incoming request raises an
   orchestration pairing approval (approval-card or notification path), showing `agentId`,
   resolved handle/profile if any, and first-seen time.
3. User accepts → core calls `POST /contacts/{sessionId}/accept`; decline → `DELETE`; block →
   `…/block`. Config `[orchestration] auto_accept_session_contacts = false` (default **off**;
   never auto-accept arbitrary requests).

## Deliverables

1. **Core contacts controllers** (`src/openhuman/tinyplace/schemas.rs` + `ops.rs`, internal
   registry): `tinyplace_contacts_{request,accept,remove,block,unblock,list,requests,status,stats}`
   mapping 1:1 onto the routes above through `TinyPlaceState::client()` signed HTTP.
2. **Pairing manager** (`orchestration/pairing.rs`): poll or stream incoming contact requests
   (cursor in the stage-3 store's `kv`), dedupe, raise/resolve approvals, persist pairing records
   `{ agent_id, label?, status, linked_at, source: user_link | approved_request }`. Publishes
   `DomainEvent::OrchestrationPairingChanged`.
3. **Renderer client**: `contacts` namespace in `invokeApiClient.ts`
   (`openhuman.tinyplace_contacts_*`) mirroring the SDK types.
4. **UI (Brain tab)**: "Link a session" affordance (paste `@handle`/agentId → request + pending
   chip → accepted), pending-request approval list (accept / decline / block), and a linked-
   sessions indicator on session chat windows (unlinked/pending sessions render a "waiting for
   pairing" state instead of messages). i18n keys across all 14 locales.
5. **Stage-1 handshake contract (amend `stage-01`)**: wrapper checks
   `contacts/{owner}/status` on start; `none` → send request + queue (bounded) + poll;
   `pending incoming` from the configured owner → accept; `accepted` → forward; `blocked` → hard
   error. Forwarder branches on the `not_a_contact` error code at send time and re-enters pairing
   instead of retrying blindly.
6. **Security invariants**: wrapper auto-accepts only its configured owner identity; core never
   auto-accepts unless the user initiated the link (flow A) or explicitly enabled the config;
   blocked identities are never re-requested automatically; the contact graph is private — don't
   sync it to any store outside the workspace.

## Tasks

1. Core controllers + ops with mock-relay unit tests (status transitions, idempotent request,
   blocked refusal).
2. Pairing manager + approval integration + events; tests for flow A (crossing auto-accept) and
   flow B (approve/decline/block), dedupe on re-poll.
3. Renderer `contacts` namespace + Vitest (RPC name mapping, type round-trip).
4. Brain-tab UX + i18n + component tests (link flow optimistic states, pending approval list,
   unpaired session placeholder).
5. Amend the stage-1 wrapper per deliverable 5 (lands in `tiny.place/` with its own tests: queue
   bounds, owner-only auto-accept, `not_a_contact` recovery).
6. `tests/json_rpc_e2e.rs`: pair → DM flows; unpaired → send refused surfaced cleanly.

## Acceptance criteria

- Flow A: pasting a wrapper identity in the tab yields an accepted edge (crossing-request case
  covered) and envelopes flow with no approval prompt.
- Flow B: an unsolicited session request appears as a pending approval; accept → DMs flow;
  decline/block → wrapper reports a clear terminal state and stops retrying.
- `not_a_contact` at send time never loops hot — it parks into pairing state on both sides.
- No path auto-accepts an arbitrary identity; config default is prompt.
- `pnpm test:rust`, `pnpm test`, `pnpm i18n:check` green; ≥80% changed-line coverage.
