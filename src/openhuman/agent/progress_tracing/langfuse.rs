//! Langfuse ingestion exporter for agent trace spans (issue #4249 follow-up).
//!
//! When `[observability.agent_tracing]` has `enabled = true` and
//! `backend = "langfuse"`, a completed run's spans are POSTed to the OpenHuman
//! backend's Langfuse **proxy** route, `/telemetry/langfuse/ingestion`, derived
//! from the **current backend hostname** (`effective_backend_api_url`). The
//! request reuses the OpenHuman **session bearer** — the same auth every other
//! backend call carries; the backend authenticates that JWT, injects the
//! Langfuse project keys server-side, and forwards the batch to Langfuse's real
//! `/api/public/ingestion` (backend `src/services/langfuseProxy.ts`). Clients
//! never hold Langfuse keys and never hit `/api/public/ingestion` directly.
//!
//! Best-effort: any failure is logged and swallowed by the caller so tracing
//! never breaks a turn. By default spans carry only metadata (names, kinds,
//! timings, and non-PII token/cost figures — the latter promoted into Langfuse's
//! native `usageDetails`/`costDetails`). Prompt text and the model's reply are
//! withheld unless the operator opts in via
//! `observability.agent_tracing.capture_content`, preserving the project's
//! "never log secrets or full PII" default.

use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::api::config::effective_backend_api_url;
use crate::api::jwt::bearer_authorization_value;
use crate::openhuman::config::Config;
use crate::openhuman::credentials::session_support::require_live_session_token;

use super::{SpanStatus, TraceSpan};

const LOG_TARGET: &str = "agent-tracing::langfuse";
/// Backend proxy route for Langfuse ingestion (relative to the backend origin).
/// The backend authenticates the caller's session JWT, injects the Langfuse
/// project keys, and forwards to Langfuse's real `/api/public/ingestion` — so
/// clients POST here, NOT to `/api/public/ingestion` (which is unexposed and
/// carries no keys).
const INGESTION_PATH: &str = "/telemetry/langfuse/ingestion";
/// Cap the push so a slow/hung Langfuse never stalls run teardown.
const PUSH_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolve the Langfuse ingestion URL from the current backend host. Joins the
/// proxy path onto [`effective_backend_api_url`] — the exact base-server
/// resolution every other backend call uses — via the canonical
/// [`crate::api::config::api_url`] helper, which replaces any path the base
/// carried with the given absolute path. So the host always matches wherever the
/// app's domain calls go (staging, prod, or a custom `api_url` override).
pub(crate) fn ingestion_url(config: &Config) -> String {
    let base = effective_backend_api_url(&config.api_url);
    crate::api::config::api_url(&base, INGESTION_PATH)
}

/// Epoch-milliseconds → RFC 3339 / ISO-8601 string (Langfuse requires ISO
/// timestamps, not epoch integers). Falls back to "now" only if the value is
/// somehow out of range — `start_unix_ms` comes from a monotonic wall clock so
/// this is defensive.
fn iso_millis(unix_ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(unix_ms as i64)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339()
}

/// Langfuse observation level for a span status. Only `Error` is elevated so
/// failed tool calls / turns surface in the Langfuse UI.
fn level_for(status: SpanStatus) -> &'static str {
    match status {
        SpanStatus::Error => "ERROR",
        SpanStatus::Ok | SpanStatus::Unset => "DEFAULT",
    }
}

/// Build the Langfuse `metadata` object from the span's (secret-free)
/// attributes plus its structured kind.
fn langfuse_metadata(span: &TraceSpan) -> Value {
    let mut map = Map::new();
    for (key, value) in &span.attributes {
        map.insert(key.clone(), value.clone());
    }
    if let Ok(kind) = serde_json::to_value(span.kind) {
        map.insert("kind".to_string(), kind);
    }
    Value::Object(map)
}

/// Convert finished spans into a Langfuse `/api/public/ingestion` batch payload:
/// a single `trace-create` for the shared trace id followed by one
/// `span-create` observation per span. Field names are Langfuse's camelCase
/// (`traceId`, `startTime`, `parentObservationId`); timestamps are ISO strings.
pub(crate) fn spans_to_langfuse_batch(spans: &[TraceSpan], include_content: bool) -> Value {
    let mut batch: Vec<Value> = Vec::with_capacity(spans.len() + 1);

    // One trace-create for the run, keyed by the shared trace id. Prefer the
    // root (parentless) span for the trace name/start; fall back to the first.
    if let Some(root) = spans
        .iter()
        .find(|s| s.parent_span_id.is_none())
        .or_else(|| spans.first())
    {
        let mut trace_body = json!({
            "id": root.trace_id,
            "name": root.name,
            "timestamp": iso_millis(root.start_unix_ms),
        });
        // Attribute the trace to the user and group per-turn traces under the
        // conversation via Langfuse's native `userId`/`sessionId` (read from the
        // turn span's stamped attributes).
        if let Some(user) = root.attributes.get("user.id").and_then(Value::as_str) {
            trace_body["userId"] = json!(user);
        }
        if let Some(group) = root.attributes.get("thread.id").and_then(Value::as_str) {
            trace_body["sessionId"] = json!(group);
        }
        batch.push(json!({
            "id": new_event_id(),
            "type": "trace-create",
            "timestamp": iso_millis(root.start_unix_ms),
            "body": trace_body,
        }));
    }

    for span in spans {
        let mut body = json!({
            "id": span.span_id,
            "traceId": span.trace_id,
            "name": span.name,
            "startTime": iso_millis(span.start_unix_ms),
            "metadata": langfuse_metadata(span),
            "level": level_for(span.status),
        });
        if let Some(end) = span.end_unix_ms {
            body["endTime"] = json!(iso_millis(end));
        }
        if let Some(parent) = &span.parent_span_id {
            body["parentObservationId"] = json!(parent);
        }
        // Prompt/reply content is transmitted only when the caller opted in
        // (`observability.agent_tracing.capture_content`); otherwise it never
        // leaves the device even though it may sit on the in-memory span.
        if include_content {
            if let Some(input) = &span.input {
                body["input"] = input.clone();
            }
            if let Some(output) = &span.output {
                body["output"] = output.clone();
            }
        }
        // A span carrying `gen_ai.usage.*` attributes (today only the root turn
        // span) is emitted as a Langfuse `generation` so the UI renders native
        // token usage + cost instead of burying them in metadata. Token counts
        // and cost are non-PII, so this promotion is unconditional.
        let event_type = if apply_usage_fields(&mut body, span) {
            "generation-create"
        } else {
            "span-create"
        };
        batch.push(json!({
            "id": new_event_id(),
            "type": event_type,
            "timestamp": iso_millis(span.start_unix_ms),
            "body": body,
        }));
    }

    json!({ "batch": batch })
}

/// Promote a span's `gen_ai.usage.*` / `gen_ai.request.model` attributes into
/// Langfuse's native `model` / `usageDetails` / `costDetails` fields so the
/// trace surfaces real token counts and cost (Langfuse only renders these on
/// `generation` observations). Returns `true` when usage was found, so the
/// caller emits the span as a `generation-create`. Only token/cost figures are
/// touched — never prompt text or PII.
fn apply_usage_fields(body: &mut Value, span: &TraceSpan) -> bool {
    let attrs = &span.attributes;
    let input = attrs
        .get("gen_ai.usage.input_tokens")
        .and_then(Value::as_u64);
    let output = attrs
        .get("gen_ai.usage.output_tokens")
        .and_then(Value::as_u64);
    if input.is_none() && output.is_none() {
        return false;
    }
    let input = input.unwrap_or(0);
    let output = output.unwrap_or(0);
    // `input_tokens` is INCLUSIVE of cached input (see cost.rs) but Langfuse
    // treats every `usageDetails` key as DISJOINT and sums them for aggregation
    // (issue #4454). Emitting the full `input` alongside `cache_read_input_tokens`
    // double-counts the cached slice and makes the components disagree with
    // `total`. Emit the uncached remainder as `input` so the parts are disjoint
    // and `input + cache_read + output == total`.
    let cached = attrs
        .get("gen_ai.usage.cached_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        // Defensive: never let a bad `cached > input` underflow.
        .min(input);
    let total = input.saturating_add(output);
    let mut usage = Map::new();
    usage.insert("input".to_string(), json!(input.saturating_sub(cached)));
    usage.insert("output".to_string(), json!(output));
    usage.insert("total".to_string(), json!(total));
    if cached > 0 {
        usage.insert("cache_read_input_tokens".to_string(), json!(cached));
    }
    body["usageDetails"] = Value::Object(usage);
    if let Some(model) = attrs.get("gen_ai.request.model").and_then(Value::as_str) {
        body["model"] = json!(model);
    }
    if let Some(cost) = attrs.get("gen_ai.usage.cost_usd").and_then(Value::as_f64) {
        body["costDetails"] = json!({ "total": cost });
    }
    true
}

/// Fresh per-event id. Langfuse dedupes ingestion events by this id, so it must
/// be unique per event (distinct from the observation/trace id in `body`).
fn new_event_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Push `spans` to the co-hosted Langfuse server. Resolves the endpoint from the
/// current backend host and authenticates with the live session bearer. Returns
/// `Err` (for the caller to log + fall back) when there is no live session, the
/// host is unresolvable, the request fails, or Langfuse rejects the batch.
pub(crate) async fn push_spans(config: &Config, spans: &[TraceSpan]) -> Result<(), String> {
    if spans.is_empty() {
        return Ok(());
    }
    let url = ingestion_url(config);
    if !url.starts_with("http") {
        return Err(format!(
            "could not resolve Langfuse ingestion URL from backend host (got {url:?})"
        ));
    }
    let token = require_live_session_token(config)?;
    let include_content = config.observability.agent_tracing.capture_content;
    let batch = spans_to_langfuse_batch(spans, include_content);
    let span_count = spans.len();

    tracing::debug!(
        target: LOG_TARGET,
        "[agent-tracing] pushing {span_count} spans to Langfuse at {url}"
    );

    let response = reqwest::Client::new()
        .post(&url)
        .header(
            reqwest::header::AUTHORIZATION,
            bearer_authorization_value(&token),
        )
        .timeout(PUSH_TIMEOUT)
        .json(&batch)
        .send()
        .await
        .map_err(|err| format!("POST {url} failed: {err}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let excerpt: String = body.chars().take(200).collect();
        return Err(format!("Langfuse ingestion returned {status}: {excerpt}"));
    }
    // Langfuse returns 207 Multi-Status even when individual events are rejected
    // — the failures live in the response `errors` array, not the HTTP status.
    // Surface them (a partial rejection is logged but never fails the turn).
    let rejected = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v.get("errors").and_then(Value::as_array).cloned())
        .filter(|errs| !errs.is_empty());
    if let Some(errs) = rejected {
        let excerpt: String = serde_json::to_string(&errs)
            .unwrap_or_default()
            .chars()
            .take(400)
            .collect();
        tracing::warn!(
            target: LOG_TARGET,
            "[agent-tracing] Langfuse ({status}) rejected {} of {span_count} span event(s): {excerpt}",
            errs.len()
        );
    } else {
        tracing::debug!(
            target: LOG_TARGET,
            "[agent-tracing] pushed {span_count} spans to Langfuse ({status})"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::openhuman::agent::progress_tracing::SpanKind;

    fn span(
        trace: &str,
        id: &str,
        parent: Option<&str>,
        name: &str,
        kind: SpanKind,
        status: SpanStatus,
        start: u64,
        end: Option<u64>,
    ) -> TraceSpan {
        let mut attributes = BTreeMap::new();
        attributes.insert("tokens".to_string(), json!(42));
        TraceSpan {
            trace_id: trace.to_string(),
            span_id: id.to_string(),
            parent_span_id: parent.map(str::to_string),
            name: name.to_string(),
            kind,
            start_unix_ms: start,
            end_unix_ms: end,
            status,
            attributes,
            input: None,
            output: None,
        }
    }

    #[test]
    fn ingestion_url_uses_backend_origin_and_ingestion_path() {
        let mut config = Config::default();
        config.api_url = Some("https://staging-api.tinyhumans.ai/api/v1".to_string());
        assert_eq!(
            ingestion_url(&config),
            "https://staging-api.tinyhumans.ai/telemetry/langfuse/ingestion",
            "endpoint is the backend's Langfuse proxy route on the base server \
             host, replacing any inference path the base carried"
        );

        // A base carrying an inference path resolves to the proxy route on the
        // SAME host — the ingestion host tracks the base server URL, not a fixed
        // literal.
        let mut with_inference_path = Config::default();
        with_inference_path.api_url =
            Some("https://api.tinyhumans.ai/openai/v1/chat/completions".to_string());
        assert_eq!(
            ingestion_url(&with_inference_path),
            "https://api.tinyhumans.ai/telemetry/langfuse/ingestion"
        );
    }

    #[test]
    fn iso_millis_formats_epoch_as_rfc3339() {
        // 2021-01-01T00:00:00Z = 1_609_459_200_000 ms.
        assert!(iso_millis(1_609_459_200_000).starts_with("2021-01-01T00:00:00"));
    }

    #[test]
    fn batch_emits_trace_create_then_one_span_create_each() {
        let spans = vec![
            span(
                "trace-1",
                "root",
                None,
                "agent.turn",
                SpanKind::Turn,
                SpanStatus::Ok,
                1_000,
                Some(2_000),
            ),
            span(
                "trace-1",
                "tool-1",
                Some("root"),
                "tool.web_search",
                SpanKind::Tool,
                SpanStatus::Error,
                1_100,
                Some(1_500),
            ),
        ];
        let payload = spans_to_langfuse_batch(&spans, false);
        let batch = payload["batch"].as_array().expect("batch array");
        assert_eq!(batch.len(), 3, "one trace-create + two span-create");

        assert_eq!(batch[0]["type"], "trace-create");
        assert_eq!(batch[0]["body"]["id"], "trace-1");

        // Camel-case Langfuse fields, ISO timestamps, parent linkage, error level.
        let root = &batch[1];
        assert_eq!(root["type"], "span-create");
        assert_eq!(root["body"]["id"], "root");
        assert_eq!(root["body"]["traceId"], "trace-1");
        assert!(root["body"]["startTime"].as_str().unwrap().contains('T'));
        assert_eq!(root["body"]["level"], "DEFAULT");
        assert_eq!(root["body"]["metadata"]["kind"], "turn");
        assert!(root["body"].get("parentObservationId").is_none());

        let tool = &batch[2];
        assert_eq!(tool["body"]["parentObservationId"], "root");
        assert_eq!(tool["body"]["level"], "ERROR");
        assert!(tool["body"]["endTime"].as_str().unwrap().contains('T'));

        // Event ids are unique and distinct from the observation ids.
        assert_ne!(batch[1]["id"], batch[2]["id"]);
        assert_ne!(batch[1]["id"], batch[1]["body"]["id"]);
    }

    #[test]
    fn usage_span_becomes_generation_and_content_is_gated() {
        let mut turn = span(
            "trace-1",
            "root",
            None,
            "agent.turn",
            SpanKind::Turn,
            SpanStatus::Ok,
            1_000,
            Some(2_000),
        );
        turn.attributes.clear();
        turn.attributes
            .insert("gen_ai.request.model".into(), json!("claude-x"));
        turn.attributes
            .insert("gen_ai.usage.input_tokens".into(), json!(100));
        turn.attributes
            .insert("gen_ai.usage.output_tokens".into(), json!(20));
        turn.attributes
            .insert("gen_ai.usage.cost_usd".into(), json!(0.0123));
        turn.input = Some(json!("what is 2+2?"));
        turn.output = Some(json!("4"));
        let spans = vec![turn];

        // Content OFF (default): span is promoted to a generation with native
        // usage + cost, but prompt/reply are withheld.
        let off = spans_to_langfuse_batch(&spans, false);
        let obs = &off["batch"][1];
        assert_eq!(obs["type"], "generation-create");
        assert_eq!(obs["body"]["model"], "claude-x");
        assert_eq!(obs["body"]["usageDetails"]["input"], 100);
        assert_eq!(obs["body"]["usageDetails"]["output"], 20);
        assert_eq!(obs["body"]["usageDetails"]["total"], 120);
        assert_eq!(obs["body"]["costDetails"]["total"], 0.0123);
        assert!(
            obs["body"].get("input").is_none(),
            "prompt must be withheld when capture_content is off"
        );
        assert!(obs["body"].get("output").is_none());

        // Content ON: prompt/reply included, usage/cost unchanged.
        let on = spans_to_langfuse_batch(&spans, true);
        let obs = &on["batch"][1];
        assert_eq!(obs["type"], "generation-create");
        assert_eq!(obs["body"]["input"], "what is 2+2?");
        assert_eq!(obs["body"]["output"], "4");
        assert_eq!(obs["body"]["costDetails"]["total"], 0.0123);
    }

    #[test]
    fn usage_details_components_are_disjoint_and_sum_to_total() {
        // `input_tokens` is inclusive of cached input; Langfuse sums the
        // usageDetails keys, so the emitted `input` must exclude the cached
        // slice to avoid double-counting (issue #4454).
        let mut turn = span(
            "trace-1",
            "root",
            None,
            "agent.turn",
            SpanKind::Turn,
            SpanStatus::Ok,
            1_000,
            Some(2_000),
        );
        turn.attributes.clear();
        turn.attributes
            .insert("gen_ai.usage.input_tokens".into(), json!(100));
        turn.attributes
            .insert("gen_ai.usage.output_tokens".into(), json!(20));
        turn.attributes
            .insert("gen_ai.usage.cached_input_tokens".into(), json!(30));

        let batch = spans_to_langfuse_batch(&[turn], false);
        let usage = &batch["batch"][1]["body"]["usageDetails"];
        // input excludes cached; cached reported separately; total unchanged.
        assert_eq!(usage["input"], 70, "input must be non-cached remainder");
        assert_eq!(usage["cache_read_input_tokens"], 30);
        assert_eq!(usage["output"], 20);
        assert_eq!(usage["total"], 120, "total is original input + output");
        // Disjoint components sum exactly to total.
        let sum = usage["input"].as_u64().unwrap()
            + usage["cache_read_input_tokens"].as_u64().unwrap()
            + usage["output"].as_u64().unwrap();
        assert_eq!(sum, usage["total"].as_u64().unwrap());
    }

    #[test]
    fn usage_details_omits_cache_key_when_no_cached_tokens() {
        // With zero cached tokens, `input` is the full input and there is no
        // cache_read key — components still sum to total.
        let mut turn = span(
            "trace-1",
            "root",
            None,
            "agent.turn",
            SpanKind::Turn,
            SpanStatus::Ok,
            1_000,
            Some(2_000),
        );
        turn.attributes.clear();
        turn.attributes
            .insert("gen_ai.usage.input_tokens".into(), json!(100));
        turn.attributes
            .insert("gen_ai.usage.output_tokens".into(), json!(20));

        let batch = spans_to_langfuse_batch(&[turn], false);
        let usage = &batch["batch"][1]["body"]["usageDetails"];
        assert_eq!(usage["input"], 100);
        assert!(usage.get("cache_read_input_tokens").is_none());
        assert_eq!(usage["total"], 120);
    }

    #[test]
    fn trace_create_carries_user_and_session_grouping() {
        // The turn span's user.id / thread.id attributes are promoted onto the
        // trace-create as Langfuse userId / sessionId so per-turn traces group
        // under one conversation and attribute to a user.
        let mut turn = span(
            "trace:req-1",
            "root",
            None,
            "agent.turn",
            SpanKind::Turn,
            SpanStatus::Ok,
            1_000,
            Some(2_000),
        );
        turn.attributes.insert("user.id".into(), json!("client-7"));
        turn.attributes
            .insert("thread.id".into(), json!("thread-abc"));
        let payload = spans_to_langfuse_batch(&[turn], false);
        let trace = &payload["batch"][0];
        assert_eq!(trace["type"], "trace-create");
        assert_eq!(trace["body"]["userId"], "client-7");
        assert_eq!(trace["body"]["sessionId"], "thread-abc");
    }

    #[tokio::test]
    async fn empty_spans_push_is_ok_noop() {
        let config = Config::default();
        // Empty batch short-circuits before any host/token resolution or network.
        assert!(push_spans(&config, &[]).await.is_ok());
    }
}
