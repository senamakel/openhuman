# TinyChannels Integration Plan

Plan for adopting the `tinychannels` crate
(`~/work/tinyhumansai/tinychannels`) as the typed channel layer, deleting the
duplicated copies in this repo, and fixing the channel bugs surfaced by the
2026-07-04 cross-repo audit.

Companion docs in the tinychannels repo:

- `docs/spec/openclaw-hermes-channel-porting.md` — the upstream research spec.
- `docs/spec/tinychannels-execution-plan.md` — the crate-side phased plan.
  Steps below reference its phases.

## Current State

- This repo does **not** depend on tinychannels. Every ported surface is a
  duplicated copy that is already drifting (rustfmt/edition diffs today,
  semantic drift tomorrow):

  | openhuman-4 file | tinychannels file |
  | --- | --- |
  | `src/openhuman/channels/traits.rs` | `src/traits.rs` |
  | `src/openhuman/channels/controllers/definitions.rs` | `src/controllers/definitions.rs` |
  | `src/openhuman/channels/controllers/ops/types.rs` | `src/controllers/types.rs` |
  | `src/openhuman/config/schema/channels.rs` | `src/config.rs` |
  | `src/openhuman/channels/context.rs` (key/memory helpers) | `src/context.rs` |
  | `src/openhuman/channels/runtime/supervision.rs:84` (`compute_max_in_flight_messages`) | `src/runtime.rs` |

- The `ChannelBackend` trait (`tinychannels/src/backend.rs`) has no
  implementation here yet; its methods map 1:1 onto the existing free
  functions in `channels/controllers/ops/{connect,messaging,discord,
  telegram}.rs`.

## Step 1 — Add the dependency, delete duplicates

- Add `tinychannels = { path = "../tinychannels" }` to `Cargo.toml`
  (single crate, not a workspace; edition 2021 depending on an edition-2024
  crate is fine).
- Delete the duplicated definitions/types/traits/config-schema/helper code and
  re-export from the old paths so the 100+ call sites keep compiling:
  - `src/openhuman/channels/mod.rs`: `pub use tinychannels::{Channel,
    ChannelMessage, SendMessage, ...};`
  - `src/openhuman/channels/controllers/...`: re-export `ChannelDefinition`,
    `ChannelAuthMode`, response types.
  - `src/openhuman/config/schema/channels.rs`: re-export `ChannelsConfig` and
    provider config structs. Note tinychannels inlined `EmailConfig` and
    `YuanbaoConfig`, which this repo currently sources from
    `channels::email_channel` / `providers::yuanbao` — repoint those two to
    the crate and delete the local definitions.
- Delete the migrated in-module tests along with the code (they already run in
  tinychannels): definitions tests, config schema tests, traits tests,
  context key/memory tests, `compute_max_in_flight_messages` tests, plus
  `tests/memory.rs` key tests and the key assertions in
  `tests/runtime_dispatch.rs`. Keep everything provider- or app-specific
  (see Step 4).
- Do not remove sandbox/security config from this repo based on the crate's
  copy: the crate is dropping its unwired `SecurityConfig`/`SandboxConfig`
  cluster as scope creep; this repo's originals (if used) stay where they are.

## Step 2 — Implement `ChannelBackend`

- New `OpenHumanChannelBackend` in
  `src/openhuman/channels/controllers/backend_impl.rs`, delegating each trait
  method to the existing ops functions:
  - `send_message` → `messaging.rs::channel_send_message` (already composes
    `effective_backend_api_url` + `jwt::get_session_token` +
    `BackendOAuthClient`).
  - `connect_channel` / `disconnect_channel` → `connect.rs` flows plus
    `credentials::ops::{store,remove}_provider_credentials` (keyed
    `"channel:<slug>"`).
  - `channel_status` / `test_channel` → existing status/test ops plus
    `health::snapshot`.
  - Telegram login and Discord link/guild/permission methods → the existing
    `telegram.rs` / `discord.rs` ops.
- Route controller entry points through `ChannelManager<OpenHumanChannelBackend>`
  so credential validation (`ChannelDefinition::validate_credentials`) happens
  in one place.
- The event bus, health bus, and dispatch engine stay app-side and *drive*
  tinychannels. Never add a tinychannels → openhuman dependency; the
  `runtime/` dispatch engine and the `web` provider are consumers of the
  crate, not porting candidates (they import ~45 openhuman modules).

## Step 3 — Bug fixes coordinated with the crate

Fix these here in lockstep with tinychannels Phase 1/2 (same semantics, one
implementation — prefer calling the crate's new helpers over patching local
copies):

1. **UTF-16 chunking.** `providers/telegram/text.rs:4-50` and
   `providers/discord/channel.rs:111-154` split by `.chars().count()`;
   Telegram (4096) and Discord (2000) count UTF-16 code units, so emoji-heavy
   messages can exceed the platform limit. Replace both with the crate's
   Phase 2 chunker (which also adds fence preservation and `(i/N)`
   indicators).
2. **Telegram forum-topic session collapse.**
   `channels/context.rs:76-90` drops `thread_ts` for telegram, so all topics
   in a supergroup share one history/memory session. Adopt the crate's fixed
   session-key helper; plan a one-time legacy-key migration (the crate ships
   a canonicalization helper) so existing conversations keep their history.
3. **Workspace/tenant discriminator.** Session keys
   (`channels/bus.rs:1005-1042`) omit guild/team/tenant; Slack channel ids are
   only workspace-unique. Thread the new `scope_id` through inbound event
   construction (Discord guild id, Slack team id, Matrix homeserver) when
   moving to the crate's envelope.
4. **Send idempotency.** No idempotency key exists on sends; a retry after a
   transport error double-posts. Adopt the crate's
   `ChannelOutboundIntent.idempotency_key` at the `ChannelBackend` seam.
5. **`conversation_memory_key` intent check.** It keys on `msg.id`
   (per-message, not per-conversation) and this repo's tests assert that
   behavior (`tests/memory.rs`). Confirm whether per-turn keying is intended;
   the crate will rename or fix accordingly — this repo should follow.

## Step 4 — Test split

- **Migrate to tinychannels** (delete here once the crate has them):
  `controllers/schemas_tests.rs` (33), the type-shape halves of
  `controllers/ops_tests.rs` (47, rewritten against a mock `ChannelBackend`),
  and the already-mirrored suites listed in Step 1.
- **Stay here:** all `providers/*_tests.rs` (telegram 148, web 93, imessage
  58, discord 55+23, email 50, lark 42, whatsapp 41+29, irc 37, mattermost
  33, signal 32, presentation 29, linq 23, qq 11), `bus_tests.rs`,
  `routes_tests.rs`, `runtime/dispatch_tests.rs`, `runtime/startup_tests.rs`,
  and the harness-driven `tests/` integration files (prompt, telegram/discord
  integration, runtime tool calls, health, identity).
- The REST-wiring halves of `ops_tests.rs` become the test bed for
  `OpenHumanChannelBackend` (Step 2).

## Step 5 — Provider extraction ladder (later, tracks crate Phase 6)

Move provider wire code into tinychannels only when its dependencies reduce to
crate traits:

1. **Now portable (no cross-module imports):** email, irc, yuanbao, cli,
   imessage, mattermost, qq, dingtalk, presentation.
2. **After a configured-HTTP-client trait** (replaces
   `config::build_runtime_proxy_client` / `apply_runtime_proxy_to_builder`):
   discord, slack, whatsapp, lark, signal.
3. **After approval, voice/STT, pairing, and conversation-memory traits:**
   telegram (imports `approval`, `voice::create_stt_provider`,
   `memory_conversations`, `security::pairing`).
4. **Never:** `web` provider and `runtime/` dispatch — they are the harness
   front-end and orchestrator that consume the crate.

## Sequencing and risk notes

- Land Step 1 and Step 2 together in one PR if possible: adding the dep while
  keeping duplicates invites split-brain imports (some call sites on the
  crate types, some on the local copies — Rust will treat them as different
  types).
- Session-key changes (Step 3, items 2–3) change conversation identity. Ship
  them behind the crate's legacy-key canonicalization with a migration test
  that maps a sample of existing keys to new keys losslessly.
- Positive invariants to preserve (verified in the audit): no secrets are
  logged in the channels tree, and no blocking calls exist in async provider
  paths. Any moved code must keep both properties.
