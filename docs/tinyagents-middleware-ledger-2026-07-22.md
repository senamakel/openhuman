# TinyAgents middleware ownership ledger (2026-07-22)

**Status:** audited against OpenHuman WP-0 (`3ebe8d058`) and TinyAgents 2.1.
This is the WP-5 per-middleware disposition required by
`tinyagents-migration-plan-2026-07-22.md`.

## Decision rule

A middleware moves to TinyAgents only when its behavior is portable without
OpenHuman configuration, event types, persistence, billing, security policy,
tool names, or user-facing protocol. OpenHuman keeps policy and projects it
onto crate mechanics. A crate primitive already being used does not make the
host adapter duplicate code.

## Ledger

| OpenHuman middleware | Disposition | Evidence and exit condition |
| --- | --- | --- |
| `TranscriptSnapshotMiddleware` | KEEP host | Mirrors crate messages into OpenHuman `ChatMessage` persistence on failed subagent runs. Delete only if the crate returns a partial transcript on errors. |
| `OpenHumanToolExposureShadowMiddleware` | DELETE after cutover | It already composes crate `ToolAllowlistMiddleware` and `ContextualToolSelectionMiddleware`; it only compares their answer with the host precomputed registry. Flip ownership after the divergence signal so the host supplies inputs and the crate selection result becomes authoritative. |
| `HandoffMiddleware` | SPLIT | Result handoff/cursoring is generic; `ResultHandoffCache`, `extract_from_result`, IDs, and product wording are host contracts. Upstream an optional result-store/cursor hook only when another consumer exists; otherwise keep this cheap adapter. |
| `SuperContextMiddleware` | KEEP host | Runs OpenHuman's `context_scout`, registers product prepared-context state, and injects an OpenHuman prompt protocol. |
| `PromptCacheSegmentMiddleware` | KEEP seam | Compensates for OpenHuman constructing `ModelRequest` directly. TinyAgents already owns `PromptCacheGuardMiddleware`; delete this adapter when request construction moves behind the crate prompt builder. |
| `ToolOutputMiddleware` | SPLIT | TinyAgents `ToolPolicy` already carries `max_result_bytes`; generic cap enforcement belongs there. TokenJuice, semantic summarization, artifact persistence, workflow-proposal exemptions, and sampling-tool contracts stay host-side. |
| `ApprovalSecurityMiddleware` | KEEP host | Owns `ApprovalGate`, origin binding, OpenHuman `SecurityPolicy`, and product denial semantics. Crate human-approval middleware cannot widen this decision. |
| `CliRpcOnlyMiddleware` | KEEP host | `ToolScope` describes OpenHuman entry points, not portable execution safety. WP-4 centralizes its matrix host-side. |
| `CredentialScrubMiddleware` | SPLIT | TinyAgents has `RedactionMiddleware`, but OpenHuman recursively scrubs structured `raw` values and applies product credential patterns before persistence and outcome capture. Upstream structured-result redaction, then keep only host patterns/configuration. |
| host `ToolPolicyMiddleware` | KEEP host; rename later | Despite the shared name, this evaluates OpenHuman's argument-sensitive policy and approval decision. The crate middleware enforces static SDK `ToolPolicy`; both are required at different layers. Rename the host type to `OpenHumanToolPolicyMiddleware` when touching registration order. |
| `ToolOutcomeCaptureMiddleware` | KEEP host | Projects final capped/redacted results into OpenHuman `ToolCallRecord` and event/usage state. |
| `ArgRecoveryMiddleware` | UPSTREAM | JSON-string/fenced-object recovery and empty-object coercion are provider-neutral harness behavior. Add a crate argument-normalization hook before schema validation, port the regression cases, then delete the host middleware. |
| `SchemaGuardMiddleware` | ADOPTED; DELETE | TinyAgents 2.1 already has `InvalidArgsPolicy::ReturnToolError` in admission. WP-5 enables it and deletes the host pre-validation, schema-valid stub synthesis, pending map, and wrap short-circuit. |
| `MemoryProtocolMiddleware` | KEEP host | Enforces the OpenHuman read/dedupe/write/`MEMORY.md` product protocol and names specific memory tools. |
| `CostBudgetMiddleware` | SPLIT | Crate `BudgetMiddleware` owns token accumulation; OpenHuman remains authoritative for USD pricing, daily/monthly product limits, global tracker state, and pre-spend denial. Delete only the token-parity shadow after it is clean. |
| `RepeatedToolFailureMiddleware` | KEEP thin driver | The escalation ladder is already crate-owned by `NoProgressTracker`. OpenHuman still maps verdicts to steering, halt summaries, user-actionable connection wording, and two workflow body-level failure conventions. |
| `RepeatProgressMiddleware` | UPSTREAM core; keep lowering | Extend crate no-progress tracking with successful identical-call and identical-output streaks plus configurable polling exemptions. Host keeps the mapping to its halt-summary slot and steering lifecycle. |
| `ImageAwareMessageTrimMiddleware` | UPSTREAM | Image token pricing, never-dropping system messages, order preservation, orphan-tool-result repair, and proportional output reserve are generic correctness fixes for crate `MessageTrimMiddleware`. Port tests first, adopt the enhanced crate trim, then delete this replacement. |

`TurnContextMiddleware` and `HandoffConfig` are installers/config bundles rather
than execution policies. They remain the seam composition root and shrink as
the rows above move.

## Upstream PR order

1. Argument normalization: upstream the still-useful JSON-string/fenced-object
   recovery from `ArgRecoveryMiddleware`; schema-invalid recovery is already
   crate-owned through `InvalidArgsPolicy::ReturnToolError`.
2. Message trimming parity: port the image/system/order/reserve tests and fix
   crate `MessageTrimMiddleware`.
3. Successful-repeat progress detection: extend `NoProgressTracker` with
   batch-aware success signatures and configurable exemptions.
4. Structured redaction and generic result-byte enforcement, if the WP-4
   structured-result boundary demonstrates a second crate consumer.

Each upstream slice lands with crate tests before the host pointer/cutover.
Host security, billing, persistence, and product-protocol middlewares are not
blocked on those PRs and are not migration debt.

## Exit checks

- Every remaining host middleware row is tied to a named product contract.
- No generic recovery or trimming algorithm remains duplicated after its crate
  cutover.
- Registration-order tests prove raw results are scrubbed before capture and
  invalid arguments are rejected before approval/security sees rewritten data.
- The crate no-progress tracker owns detection; the host owns only product
  steering and presentation.
