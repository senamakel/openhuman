# 05 — Native Anthropic Provider + Prompt-Cache Request Shaping

Goal: a feature-flagged native Anthropic Messages-API provider in the crate
(the spec already reserves the slot, `docs/spec/README.md:279-284`,
`providers/types.rs:322-337`), plus wiring the existing cache-segment
metadata to real wire fields. Highest-leverage provider gap: today
`anthropic()` is an OpenAI-compat preset that silently loses prompt caching,
thinking, and cache-write accounting.

## Why this matters to OpenHuman

- Claude-family models are primary; every turn without `cache_control`
  breakpoints pays full input-token price on a growing transcript.
- Extended thinking + tool use cannot work correctly at all without
  signature replay (see 02 §4).
- `Usage.cache_creation_tokens` is structurally present but always 0 —
  OpenHuman's cost accounting (C3 flip) undercounts cache writes.
- OpenHuman's own `inference/` layer already handles this correctly; a
  native crate provider is the precondition for ever routing provider calls
  through the crate harness (the standing `reliable.rs` gate, old-plan 02.2).

## Step 1 — `providers/anthropic/` behind `feature = "anthropic"`

- Messages API: `system` as top-level blocks, `tools` array, `tool_choice`,
  `max_tokens` required, `anthropic-version` header; reuse the crate's
  reqwest/rustls stack. No new deps.
- Response mapping → `ModelResponse`: content blocks (text, tool_use,
  thinking, redacted_thinking per doc 02), `stop_reason`, and usage incl.
  `cache_creation_input_tokens` / `cache_read_input_tokens` →
  `Usage.cache_creation_tokens` / `cache_read_tokens`.
- SSE streaming: `message_start` / `content_block_start` /
  `content_block_delta` (`text_delta`, `input_json_delta`,
  `thinking_delta`, `signature_delta`) / `content_block_stop` /
  `message_delta` — mapped onto `ModelStreamItem` incl. tool-name-on-start
  (`content_block_start` carries the tool name — doc 02 §3 / doc 03 §3 get
  this for free on Anthropic).
- `ProviderKind::Anthropic` switches from preset to native when the feature
  is on; preset remains as fallback for compat gateways.

## Step 2 — `cache_control` shaping from `cache_segments`

`ModelRequest` already carries `cache_segments` / `prompt_fingerprint` /
`cacheable_prefix_ids()` (`model/types.rs:385-405`) — currently ignored by
translation. Plan:

- Translation places up to 4 `cache_control: {type: "ephemeral"}`
  breakpoints at segment boundaries: tools block, system tail, and the two
  highest-value conversation prefixes (mirrors OpenHuman's proven layout).
- `PromptCacheGuardMiddleware` (`middleware/mod.rs:19-60`) is promoted from
  observer to enforcement partner: layout-change events now correspond to
  actual cache invalidation, so its warnings become actionable.
- OpenAI path: no wire field needed (implicit prefix caching), but #04 §5's
  stable-prefix memo guarantees byte-stable prefixes — document that as the
  OpenAI half of "cache-aware shaping".
- TTL/beta options (e.g. 1h cache) via `provider_options` passthrough.

## Step 3 — Capability & catalog integration

- `ModelProfile` gains `supports_thinking`, `supports_cache_control`,
  cache-write pricing multipliers; `ModelCatalog` entries for the Claude
  family map cache read/write token prices into `CostTotals` (feeds
  old-plan C3's "pricing table wired" criterion and sdk-gaps §7/§8).

## Step 4 — Tests

- Wire-fixture tests: request JSON golden files (system/tools/cache_control
  placement, thinking replay with signatures); SSE fixture → accumulator →
  message + usage assertions (cache read/write, thinking tokens).
- `live_anthropic.rs` behind `ANTHROPIC_API_KEY`: cache-hit-on-second-call,
  multi-turn thinking + tool-use replay.

## Step 5 — OpenHuman follow-through (deliberately conservative)

- Do **not** immediately reroute OpenHuman's turn traffic — `inference/`
  stays authoritative (credentials, billing classification, OAuth).
- First consumer: sub-agent/background/eval traffic behind a flag, comparing
  cost + cache-hit telemetry against `inference/` for the same models.
- Re-evaluate the `reliable.rs` (900) verdict and old-plan 02.2 only after
  divergence-free telemetry; that decision gets its own migration note.

Effort: **L→XL** (Step 1–2 ≈ 2 weeks incl. tests; Steps 3–5 incremental).
Depends on doc 02 (thinking blocks in `ContentBlock`) for full value; can
land Step 1 text/tool support before thinking if 02 lags.
