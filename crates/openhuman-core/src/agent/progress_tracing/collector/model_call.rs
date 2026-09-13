//! Folding a per-call `ModelCallCompleted` event into a generation span plus
//! its rollups onto the enclosing subagent/turn span.

use std::collections::BTreeMap;

use crate::agent::progress_tracing::serialize::{
    capture_model_content, json_f64, json_str, json_u32, json_u64,
};
use crate::agent::progress_tracing::types::{SpanKind, SpanStatus};

use super::state::SpanCollector;

impl SpanCollector {
    /// Fold a per-call `ModelCallCompleted` into the tree:
    ///
    /// 1. emit a closed [`SpanKind::Generation`] span (name `llm.<model>`)
    ///    parented under the current iteration — or, for a child call
    ///    (`subagent_task_id` set), under the owning subagent's current
    ///    iteration — carrying exact per-call model/usage/cost plus provenance
    ///    (`gen_ai.provider`) and the pricing basis the local estimator uses;
    /// 2. record the captured request messages (incl. the system prompt) and
    ///    completion as the generation's input/output, gated on
    ///    `capture_content` and truncated to [`crate::agent::progress_tracing::serialize::MAX_MODEL_CONTENT_CHARS`];
    /// 3. accumulate reasoning / cache-creation tokens onto the root turn
    ///    span, which `TurnCostUpdated` (cumulative rollup) does not carry —
    ///    and, for child calls, roll model + usage onto the subagent span so
    ///    a delegation (e.g. the Context Scout) surfaces its model natively.
    ///
    /// The Langfuse-facing model label is `{provider_id}.{model}` (e.g.
    /// `managed.chat-v1`, `openai.gpt-4o`).
    ///
    /// Generation start is approximated by the enclosing iteration span's
    /// start (the iteration opens on `ModelStarted`); end is the observation
    /// time of the usage record.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::agent::progress_tracing) fn record_model_call(
        &mut self,
        model: &str,
        provider_id: &str,
        subagent_task_id: Option<&str>,
        input: Option<&serde_json::Value>,
        output: Option<&serde_json::Value>,
        iteration: u32,
        input_tokens: u64,
        output_tokens: u64,
        cached_input_tokens: u64,
        cache_creation_tokens: u64,
        reasoning_tokens: u64,
        cost_usd: f64,
        now_unix_ms: u64,
    ) {
        // Resolve the parent + start basis: a child call nests under its
        // subagent's current iteration (else the subagent span itself); a
        // parent call nests under the turn's current iteration (else root).
        let subagent_state = subagent_task_id.and_then(|id| self.subagents.get(id));
        let (parent, start_basis_index) = match subagent_state {
            Some(state) => match &state.current_iteration_span_id {
                Some(id) => {
                    let idx = self.span_index_by_id(id);
                    (id.clone(), idx)
                }
                None => (
                    self.spans[state.span_index].span_id.clone(),
                    Some(state.span_index),
                ),
            },
            None => {
                let parent = self.active_parent_id(now_unix_ms);
                (parent, self.current_iteration_index)
            }
        };
        let start_unix_ms = start_basis_index
            .and_then(|idx| self.spans.get(idx))
            .map(|span| span.start_unix_ms)
            .unwrap_or(now_unix_ms);

        let labeled_model = format!("{provider_id}.{model}");
        let pricing = crate::agent::cost::lookup_pricing(model);

        let mut attrs = BTreeMap::new();
        attrs.insert("gen_ai.request.model".to_string(), json_str(&labeled_model));
        attrs.insert("gen_ai.provider".to_string(), json_str(provider_id));
        attrs.insert("agent.iteration".to_string(), json_u32(iteration));
        attrs.insert(
            "gen_ai.usage.input_tokens".to_string(),
            json_u64(input_tokens),
        );
        attrs.insert(
            "gen_ai.usage.output_tokens".to_string(),
            json_u64(output_tokens),
        );
        // Cache reads always flow (even 0) so usageDetails stay complete.
        attrs.insert(
            "gen_ai.usage.cached_input_tokens".to_string(),
            json_u64(cached_input_tokens),
        );
        if cache_creation_tokens > 0 {
            attrs.insert(
                "gen_ai.usage.cache_creation_tokens".to_string(),
                json_u64(cache_creation_tokens),
            );
        }
        if reasoning_tokens > 0 {
            attrs.insert(
                "gen_ai.usage.reasoning_tokens".to_string(),
                json_u64(reasoning_tokens),
            );
        }
        attrs.insert("gen_ai.usage.cost_usd".to_string(), json_f64(cost_usd));
        // Pricing basis so Langfuse cost figures are auditable against the
        // client-side estimator (USD per million tokens).
        attrs.insert(
            "gen_ai.pricing.input_per_mtok_usd".to_string(),
            json_f64(pricing.input_per_mtok_usd),
        );
        attrs.insert(
            "gen_ai.pricing.cached_input_per_mtok_usd".to_string(),
            json_f64(pricing.cached_input_per_mtok_usd),
        );
        attrs.insert(
            "gen_ai.pricing.output_per_mtok_usd".to_string(),
            json_f64(pricing.output_per_mtok_usd),
        );

        log::debug!(
            "[agent-tracing] generation span model={labeled_model} \
             iteration={iteration} child={} in={input_tokens} out={output_tokens} \
             cost_usd={cost_usd:.6} input_captured={} output_captured={}",
            subagent_task_id.is_some(),
            input.is_some(),
            output.is_some(),
        );
        let (_, index) = self.open_span(
            SpanKind::Generation,
            format!("llm.{model}"),
            Some(parent),
            start_unix_ms,
            attrs,
        );
        // Captured request messages (incl. system prompt) + completion become
        // the generation's input/output — only while content capture is on,
        // truncated so one huge context window can't bloat the trace batch.
        if self.ctx.capture_content {
            if let Some(span) = self.spans.get_mut(index) {
                if let Some(value) = input {
                    span.input = Some(capture_model_content(value));
                }
                if let Some(value) = output {
                    span.output = Some(capture_model_content(value));
                }
            }
        }
        self.close_span(index, now_unix_ms, SpanStatus::Ok, BTreeMap::new());

        // Child call: roll model + usage + cost onto the owning subagent span
        // so the delegation row (e.g. `subagent.Context Scout`) natively shows
        // which model served it and what it cost.
        if let Some(state_index) = subagent_task_id
            .and_then(|id| self.subagents.get(id))
            .map(|state| state.span_index)
        {
            if let Some(span) = self.spans.get_mut(state_index) {
                span.attributes
                    .insert("gen_ai.request.model".to_string(), json_str(&labeled_model));
                span.attributes
                    .insert("gen_ai.provider".to_string(), json_str(provider_id));
                for (key, add) in [
                    ("gen_ai.usage.input_tokens", input_tokens),
                    ("gen_ai.usage.output_tokens", output_tokens),
                    ("gen_ai.usage.cached_input_tokens", cached_input_tokens),
                ] {
                    let prior = span
                        .attributes
                        .get(key)
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0);
                    span.attributes
                        .insert(key.to_string(), json_u64(prior.saturating_add(add)));
                }
                let prior_cost = span
                    .attributes
                    .get("gen_ai.usage.cost_usd")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0);
                span.attributes.insert(
                    "gen_ai.usage.cost_usd".to_string(),
                    json_f64(prior_cost + cost_usd),
                );
            }
            return;
        }

        // Root rollup for the usage dimensions the cumulative TurnCostUpdated
        // event does not carry (reasoning / cache-creation), plus provenance
        // and the provider-labeled model (TurnCostUpdated only knows the raw
        // model handle and can fire before the first per-call event).
        let root = match self.turn_span_index {
            Some(idx) => idx,
            None => {
                self.ensure_turn_span(now_unix_ms);
                self.turn_span_index.expect("turn span just created")
            }
        };
        if let Some(span) = self.spans.get_mut(root) {
            span.attributes
                .insert("gen_ai.provider".to_string(), json_str(provider_id));
            span.attributes
                .insert("gen_ai.request.model".to_string(), json_str(&labeled_model));
            for (key, add) in [
                ("gen_ai.usage.reasoning_tokens", reasoning_tokens),
                ("gen_ai.usage.cache_creation_tokens", cache_creation_tokens),
            ] {
                if add == 0 && !span.attributes.contains_key(key) {
                    continue;
                }
                let prior = span
                    .attributes
                    .get(key)
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                span.attributes
                    .insert(key.to_string(), json_u64(prior.saturating_add(add)));
            }
        }
    }
}
