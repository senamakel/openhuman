> Current requirement (user approved 2026-10-21): replace the fixed line target with verified ownership boundaries and all acceptance tests, including changed-line coverage >=80%. Counts below are a historical snapshot. PR542 subsequently consolidated the subagent mapper; current OH b9c191/746e964 web_chat count is 9,301 non-test Rust lines across38files. This note does not establish acceptance or close135.

# OpenHuman #135 remaining web_chat ownership audit

Audit source: worktree `/home/enamakel/work/openhuman/worktrees/epic-7964-135-progress-ledger`, OH HEAD `cdb2fdbfa22f0d37fe5780592ac577a47cc81dcd`, actual TinyAgents PR151 merge pin `0a0e354917625c26c9bc4cf34ea88cd424f77cc3`. Counts are physical lines in each non-test `.rs` file under `crates/openhuman-core/src/web_chat`; tests/support directories excluded. Recounted after the owner pin and local unused-binding cleanup: 38 files, 9,448 lines. This is a historical source ownership audit. Its numeric requirement was subsequently superseded by the user-approved ownership and acceptance requirement recorded above.

Compared with exact fork epic base `1bd49f5beef034312b4febc1b3ee8c6ef4bd0d22`, the same 38-file method counts 9,938 physical lines; this branch is 9,448 lines, a 490-line reduction. Issue #135's stated “what stays” list accounts for about 3,984 physical lines in this tree: session (623), TurnHost/run_task (750), reply persistence (96), web error classifier and helpers (710), budget correlation (174), system turns (110), journal shadow (176), trace sink (89), egress surface (107), DomainEvent subscribers (698), and RPC schemas (451). The remaining 5,464 lines are still above the target of approximately 4,500 **for the whole module**. A web-only count does not establish acceptance, and no lines were moved solely to change this count.

| File | LOC | Ownership verdict |
|---|---:|---|
| `turn_host.rs` | 750 | **Expected host port.** Implements TinyAgents `TurnHost` by resolving OpenHuman config/session identity, acquiring tenant/session/profile locks, applying origin/approval/workspace/attachment policy, invoking the agent, and retaining OpenHuman voice, budget, memory, and persisted-turn context. The agent loop and queue lifecycle already run through TinyAgents `TurnRunner` / `AgentTurnHost`; no duplicate model loop found. |
| `event_bus.rs` | 698 | **Expected host subscribers.** Converts OpenHuman `DomainEvent` values (approval, artifact, memory, agent, goals, queue) into product web events while applying host routing and data policy. The source event types and policy belong to OpenHuman. |
| `session.rs` | 623 | **Expected host port.** Uses OpenHuman `Config`, provider factory, identity, per-thread fingerprint, and scoped session host to build/reuse the agent. No generic TinyAgents cache contract with equivalent host-config invalidation was found. |
| `turn_events.rs` | 571 | **Expected host EventSink/FailureClassifier/ReplyStore/Summarizer.** Failure copy and Sentry suppression are product behavior; replies must persist before terminal notification; grounded closes use the turn's own OpenHuman config and locale. |
| `ops/start_chat.rs` | 569 | **Host RPC admission/policy.** Validates product submission, queue mode/media compatibility, guardrails, approvals, locale and deadlines before constructing the generic runner request. Not a generic runner duplicate. |
| `progress.rs` | 522 | **Host web projection adapter.** Maps typed progress to OpenHuman web UX, maintains web-specific interim/cost/timing behavior, mirrors OpenHuman turn state, and exports product traces. Generic run-ledger folding is now in TinyAgents. |
| `ops/channel_ops.rs` | 496 | **Host RPC/domain adapter.** Exposes channel.web_* controller operations, resolves caller scope and attachment/chat APIs, and maps runner admission into the public product result. |
| `schemas.rs` | 451 | **Expected host RPC schemas.** Declares OpenHuman's `channel.web_*` surface. |
| `web_errors/classify.rs` | 410 | **Expected host failure policy.** Maps provider/harness failures to OpenHuman user copy, structured failure fields, and retry semantics. |
| `voice_notes.rs` | 363 | **Expected host voice behavior.** Coordinates product voice-note persistence, reply audio/viseme metadata, and legacy compatibility. |
| `turn_close.rs` | 358 | **Expected host product close.** Builds grounded user-facing recovery using OpenHuman inference configuration and persisted session state; not harness lifecycle. |
| `surface.rs` | 333 | **Host adapter to compiled TinyChannels.** Opens/projects/ends per-turn module handles, applies trusted tenant/client/thread routing, and publishes projected output to the host event stream. |
| `ingress.rs` | 307 | **Host security and media policy.** Stages attachments, applies prompt enforcement and product approval routing, and announces admission. |
| `presentation.rs` | 294 | **Host persistence/presentation policy.** Persists assistant content/citations before publishing the terminal event; ownership and sequencing are product guarantees. |
| `suggestions.rs` | 262 | **Host product UX/policy.** Generates and publishes product follow-up suggestions using turn context. |
| `ops/state.rs` | 240 | **Host thread-state RPC.** Returns product thread/run state, queue, and activity through trusted scope. |
| `runner.rs` | 198 | **Thin host composition.** Instantiates TinyAgents `TurnRunner` ports and holds per-request `DispatchCtx`, leases, admission, and result state. Does not implement queue/guard lifecycle. |
| `progress_subagent.rs` | 185 | **Concrete next extraction candidate.** It maps each generic `Subagent* AgentProgress` to the same TinyChannels `ChannelOutputEvent::Subagent` vocabulary constructed by `agent/turn_progress.rs`; web uniquely adds correlation returned by TinyAgents' `RunProgressLedger`. Consolidate the shared mapping into the existing cross-surface adapter and test web/native parity before moving. |
| `journal_shadow.rs` | 176 | **Host trace migration policy.** Compares OpenHuman's live trace with the TinyAgents journal projection and applies product dual-run telemetry policy. |
| `ops/budget_correlation.rs` | 174 | **Expected host budget policy.** Correlates provider-specific exhaustion signals across turns and controls user-visible budget copy. |
| `types.rs` | 162 | **Host RPC/runtime records.** Contains web request/result/cache metadata tied to OpenHuman host configuration. |
| `mod.rs` | 157 | **Module boundary/re-exports.** Compatibility and facade exports for existing host callers. |
| `turn_activity.rs` | 148 | **Host stop-copy projection.** Records the last progress description and resolved config/locale for grounded idle/timeout replies. The current natural-language phrases are product copy; generic guard/liveness tracking is already in TinyAgents. |
| `web_errors/backend_error_code.rs` | 117 | **Expected host classifier helper.** Normalizes backend failures into product failure categories. |
| `ops/system_turn.rs` | 110 | **Expected host port.** Runs host-authored turns on an OpenHuman thread/session and preserves product durable transcript semantics. |
| `egress_surface.rs` | 107 | **Expected host subscriber.** Applies OpenHuman transfer/egress behavior before emitting a web event. |
| `reply_persistence.rs` | 96 | **Expected host persistence facade.** Owns persisted thread-message and citation behavior. |
| `channel_event.rs` | 95 | **Host bus routing envelope.** Stamps/routes tenant context on TinyChannels web events. |
| `progress_trace.rs` | 89 | **Expected host trace sink setup.** Selects OpenHuman trace context and journal shadow configuration. |
| `proactive.rs` | 82 | **Host product event.** Publishes proactive user-facing content under trusted context. |
| `web_errors/timeout.rs` | 76 | **Expected host timeout copy/classification.** |
| `ops.rs` | 57 | **Controller aggregator.** Re-exports web-chat operations and test hooks. |
| `web_errors/budget.rs` | 45 | **Expected host budget copy/classification.** |
| `web_errors/retry.rs` | 41 | **Expected host retry policy.** |
| `event_bus_channel.rs` | 35 | **Host scoped event bus boundary.** Applies tenant identity and timestamp/sequence compatibility when broadcasting. |
| `progress_ledger.rs` | 23 | **TinyAgents host adapter.** Selects the scoped ledger store and supplies OpenHuman client/thread/source identity to the owner recorder. |
| `progress_map.rs` | 7 | **One-release compatibility re-export** to the shared OpenHuman AgentProgress/TinyChannels mapping. |
| `web_errors.rs` | 21 | **Classifier module declarations/re-exports.** |

## Cross-surface call-graph findings

- Generic turn lifecycle, queueing, cancellation, idle/wall-clock guards, and work tracking run in TinyAgents `TurnRunner`; OpenHuman's `runner.rs` and `turn_host.rs` provide product ports rather than a second runner.
- Web and native chat use the same OpenHuman `agent/turn_progress.rs` adapter to convert TinyAgents `AgentProgress` into TinyChannels `ChannelOutputEvent`. TinyChannels' runtime/module owns channel progressive delivery and web projection. The web path invokes `Surface::{begin,project,end}` over TinyBus; hosted relay/native progressive delivery invokes TinyChannels `NativeProgressiveReply`. The mapping is in OpenHuman because neither owner can depend on the other's type without a cycle.
- OpenHuman's `DomainEvent` subscriber fan-out is product-specific; typed provider reaction/read events retain the trusted, durable transcript projection path. Those are separate from a chat turn's `ChannelOutputEvent` projection.
- `progress_subagent.rs` is the one additional concrete shared-mapping candidate identified in this audit; implement/test it in a follow-up after the current #135 gates, not as a count-only relocation.
- Most remaining lines implement caller scope, OpenHuman RPC/API and event contracts, security/approval, provider and voice policy, transcript persistence, user-facing copy, or product UI state. Moving these into TinyAgents/TinyChannels would either transfer OpenHuman policy, force an owner dependency cycle, or add a redundant generic contract. This audit does not find a defensible way to remove another ~4.9k lines from this module while preserving those boundaries. The user subsequently approved replacing the numeric target with verified ownership boundaries and all acceptance tests. No size decision remains pending; full ownership and integrated acceptance verification are still required before closure.


## Current follow-up reconciliation — 2026-10-21

PR542 consolidated the identified subagent mapper into the shared host adapter (22 additions, 169 deletions); `progress_subagent.rs` is no longer an outstanding extraction candidate. The other 37 audited production files were unchanged by that consolidation; the later shared relay adapter added the crate-local re-export reconciled below. Fresh web-suite evidence at `b1c427b064` with TinyChannels `b95eaaa7` is 299 passing tests, exit 0 (`evidence/23-web-chat-direct.log/.exit`). This is partial acceptance evidence. Full JSON-RPC E2E, changed-line coverage of at least 80%, and final integrated ownership verification remain pending. Historical counts and verdicts above identify the audit source, not the final union.

### Reconciliation at fork commit 4876cec8

A fresh file/hash comparison at PR553 confirms 38 non-test Rust files and 9,302 physical lines (test and debug test hooks excluded). Thirty-six audited files are byte-identical to the original audit. The two changes are the already-reviewed shared subagent mapper consolidation and the one-line crate-local `Surface`/`TurnIds` re-export used by the shared relay adapter. The extra export explains the one-line increase since b9c19159. Evidence: `evidence/135-ownership-reconciliation-4876.json`. These counts are informational; final JSON-RPC, changed-line coverage and integrated acceptance remain required.
