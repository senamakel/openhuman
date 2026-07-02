# Stage 1 — tiny.place session bridge (SDK/CLI, `tiny.place/` repo)

## Goal command

> In the `tiny.place/` checkout (`sdk/typescript`), define a versioned **`SessionEnvelope` v1**
> message schema for harness sessions and extend the CLI wrappers so that **every semantic message
> (user and agent) from a wrapped Codex or Claude Code instance is sent as a Signal E2E DM to a
> configured tiny.place recipient** (the OpenHuman owner agent), in addition to the existing
> on-disk JSONL envelopes. Add a `tinyplace claude` wrapper mirroring `tinyplace codex`.

## Read first

- `sdk/typescript/src/cli/codex.ts` — existing wrapper: PTY proxy + session-JSONL tailing already
  produces `SemanticMessage { role: "user" | "assistant" }` and `SessionEnvelope`s written under
  `~/.tinyplace/codex-envelopes/messages/…`. This is the source of truth to forward.
- `sdk/typescript/src/agent/` — `Agent` facade (`sendMessage` does handle-resolution + Signal E2E).
- `sdk/typescript/src/messaging/`, `src/signal/` — DM send path.
- `sdk/typescript/AGENTS.md` — identifier kinds, error-code contract.

## Deliverables

1. **`SessionEnvelope` v1 wire schema** (new `sdk/typescript/src/types/session-envelope.ts`,
   exported from the root). This is the contract stages 3–7 parse; it rides inside the DM body as
   JSON (encrypted end-to-end):

   ```ts
   interface HarnessSessionEnvelope {
     v: 1;
     kind: "session_message" | "session_lifecycle";
     source: "codex" | "claude-code";
     sessionId: string;        // stable wrapper session id
     sessionLabel?: string;    // e.g. repo folder name
     workspace?: string;       // cwd of the wrapped instance
     seq: number;              // monotonic per session
     role: "user" | "assistant" | "system";
     body: string;             // the semantic message text (may be truncated, see limits)
     truncated?: boolean;
     timestamp: string;        // ISO-8601
     lifecycle?: "started" | "ended" | "error";  // kind === session_lifecycle
   }
   ```

2. **DM forwarding in the Codex wrapper**: new `--tinyplace-forward-to <@handle|messagingKey>`
   (+ env `TINYPLACE_FORWARD_TO`). When set, each `SemanticMessage` and lifecycle event is wrapped
   in a `HarnessSessionEnvelope` and sent via the Agent facade Signal DM path. Batching: flush per
   message, but coalesce assistant deltas into one message per completed turn (the tailer already
   yields whole messages). Failures are logged to stderr JSON and **never crash the wrapped CLI**;
   retry with backoff on `transient`/`rate_limited`, drop after N retries with a lifecycle `error`.
3. **`tinyplace claude` wrapper** (`sdk/typescript/src/cli/claude.ts`): same PTY-proxy shape as
   `codex.ts`, tailing Claude Code session JSONL (`~/.claude/projects/<slug>/*.jsonl`) for
   user/assistant messages; shares the envelope writer + forwarder (extract the reusable parts of
   `codex.ts` into `src/cli/session-bridge.ts` rather than copy-pasting).
4. **Size limits**: cap `body` at 8 KiB per DM (set `truncated: true`; full text stays in the local
   JSONL). Never forward raw terminal chunks — semantic messages only.
5. **Contact-pairing handshake** (wrapper half of stage 2 — the relay refuses DMs between
   non-contacts, `403 not_a_contact`): on start with a forward target, print the wrapper's own
   identity (`agentId` / `@handle`) so the user can link it from OpenHuman, then check
   `contacts/{owner}/status`:
   - `accepted` → forward; `none` → send a contact request, queue envelopes (bounded, drop-oldest
     with a lifecycle `error` note) and poll until accepted;
   - `pending incoming` **from the exact configured forward-to identity only** → accept;
   - `blocked` → terminal error, no retries.
   At send time, branch on the `not_a_contact` error code → re-enter pairing instead of retrying.
6. Docs: README section + `tinyplace describe` entries for the new flags/command.

## Tasks

1. Extract shared session-bridge module from `codex.ts` (envelope writer, session-id logic, tailer
   interface) — no behavior change; existing tests stay green.
2. Add `HarnessSessionEnvelope` type + serializer/validator (`parseHarnessSessionEnvelope` for
   consumers) with unit tests round-tripping v1 and rejecting unknown `v`.
3. Implement the forwarder (queue + retry + never-crash guarantee) behind `--tinyplace-forward-to`.
4. Implement `claude.ts` tailer: resolve session file for the wrapped instance (newest JSONL in the
   project slug dir after spawn; honor `--tinyplace-session-file` override like codex does).
5. Implement the pairing handshake (status check, request, owner-only auto-accept, bounded queue,
   `not_a_contact` recovery) in the shared session-bridge module.
6. Wire both into `cli.ts`/`commands.ts`; update help/catalog output.
7. Vitest coverage: envelope schema, forwarder retry/drop behavior (mock client), pairing state
   machine (all four contact statuses + owner-only accept), claude JSONL parsing fixtures (user
   message, assistant message, tool-use records ignored).

## Acceptance criteria

- `tinyplace codex --tinyplace-forward-to @owner -- "…" ` forwards every user + assistant message
  of the session as Signal DMs whose plaintext bodies parse as `HarnessSessionEnvelope` v1, plus
  `started`/`ended` lifecycle envelopes.
- `tinyplace claude …` does the same for a Claude Code session.
- With no contact edge, the wrapper pairs (or waits, queueing) before any envelope DM is sent; it
  never auto-accepts an identity other than the configured forward-to target.
- Killing the network mid-session does not break the wrapped CLI; envelopes on disk stay complete.
- `pnpm test` green in `sdk/typescript`; new module has no `any`-typed public surface.
