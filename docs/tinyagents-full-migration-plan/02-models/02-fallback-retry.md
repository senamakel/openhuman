# 02.2 — Retry/fallback ownership → RunPolicy

`provider/reliable.rs` (1.2k lines + 1.4k tests) wraps every provider in
retry/fallback; the crate loop ALSO retries (`RunLimits.max_retries_per_call
= 3`). Audit then collapse to one owner: the SDK.

## Steps

1. Map `ReliableProvider` behaviors onto crate primitives:
   transient 429/5xx → `RetryPolicy` (exp backoff) + `RateLimitMiddleware`;
   provider chain → `FallbackPolicy` (ordered model names, needs 02.1
   multi-registration); permanent config rejection / billing exhaustion →
   non-retryable `ProviderError { retryable: false }` mapping in
   `ProviderModel`.
2. Keep OpenHuman's error classification (`ops/http_error.rs`,
   `billing_error`, `auth_error_registry`) as the mapper that fills
   `ProviderError`; it is product knowledge.
3. Un-wrap `ReliableProvider` from `session/builder/factory.rs` (re-layered
   there 2026-06-30) once parity tests cover: transient retry count, retry
   events (`RetryScheduled`), fallback events (`FallbackSelected`), and NO
   double-retry (assert total attempt counts with `MockModel::call_count`).
4. Non-turn callers of `ReliableProvider` (subconscious, memory sync, etc.):
   either route them through a minimal harness invoke or a small shared
   retry util — inventory call sites first; do not leave a fork.

## Deletions

- `src/openhuman/inference/provider/reliable.rs` + tests (rewrite the
  behaviorally-relevant cases against RunPolicy).

## Acceptance

- Attempt-count parity matrix (429, 500, config rejection, billing) green.
- `FallbackSelected`/`RetryScheduled` visible in the event bridge.
