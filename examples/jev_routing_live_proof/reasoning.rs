//! Strict-JSON OpenHuman reasoning-router escalation over the identical snapshot.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use openhuman_core::openhuman::agent::progress::AgentProgress;
use openhuman_core::Harness;
use serde::{Deserialize, Serialize};
use tinyhivemind_embed::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, Router, RouterFuture,
    RoutingEvaluation, RoutingRequest,
};

/// One reasoning escalation, including its metering.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct ReasoningTrace {
    pub(crate) prompt: String,
    pub(crate) reply: String,
    pub(crate) latency_ms: u64,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) cached_input_tokens: u64,
    pub(crate) cost_usd: f64,
}

#[derive(Debug, Deserialize)]
struct ReasoningAnswer {
    primary_responder: String,
    primary_probabilities: Vec<CandidateProbability>,
    confidence: tinyhivemind::responder::Probability,
    needs_collaboration: tinyhivemind::responder::Probability,
    needs_clarification: tinyhivemind::responder::Probability,
    contributions: Vec<ContributionProbability>,
    high_impact: tinyhivemind::responder::Probability,
}

#[derive(Clone)]
pub(crate) struct OpenHumanReasoningRouter {
    harness: Arc<Harness>,
    traces: Arc<Mutex<Vec<ReasoningTrace>>>,
}

impl OpenHumanReasoningRouter {
    pub(crate) fn new(harness: Arc<Harness>) -> Self {
        Self {
            harness,
            traces: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn traces(&self) -> Result<Vec<ReasoningTrace>, String> {
        self.traces
            .lock()
            .map(|traces| traces.clone())
            .map_err(|_| "reasoning trace lock was poisoned".to_owned())
    }
}

impl Router for OpenHumanReasoningRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        Box::pin(async move {
            let prompt = prompt(request)?;
            let (tx, mut rx) = tokio::sync::mpsc::channel(64);
            let collector = tokio::spawn(async move {
                let mut cost = (0, 0, 0, 0.0);
                while let Some(progress) = rx.recv().await {
                    if let AgentProgress::TurnCostUpdated {
                        input_tokens,
                        output_tokens,
                        cached_input_tokens,
                        total_usd,
                        ..
                    } = progress
                    {
                        cost = (input_tokens, output_tokens, cached_input_tokens, total_usd);
                    }
                }
                cost
            });
            let started = Instant::now();
            let outcome = self
                .harness
                .turn(&prompt)
                .session("openhuman-live-proof:routing-escalation")
                .on_progress(tx)
                .send()
                .await?;
            let (input_tokens, output_tokens, cached_input_tokens, cost_usd) =
                tokio::time::timeout(std::time::Duration::from_secs(10), collector)
                    .await
                    .map_err(|_| "reasoning progress collector did not settle")??;
            let reply = outcome.reply;
            self.traces
                .lock()
                .map_err(|_| "reasoning trace lock was poisoned")?
                .push(ReasoningTrace {
                    prompt,
                    reply: reply.clone(),
                    latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    input_tokens,
                    output_tokens,
                    cached_input_tokens,
                    cost_usd,
                });
            let parsed: ReasoningAnswer = serde_json::from_str(extract_json(&reply))?;
            Ok(RoutingEvaluation {
                primary_responder: parsed.primary_responder,
                primary_probabilities: parsed.primary_probabilities,
                confidence: parsed.confidence,
                needs_collaboration: parsed.needs_collaboration,
                needs_clarification: parsed.needs_clarification,
                contributions: parsed.contributions,
                high_impact: parsed.high_impact,
                model_identity: "openhuman-reasoning-router".into(),
                question_schema_version: 1,
                roster_version: request.roster_version,
                disposition: EvaluationDisposition::Unchecked,
            })
        })
    }
}

fn prompt(request: &RoutingRequest) -> Result<String, serde_json::Error> {
    Ok(format!(
        "You are the one permitted reasoning-router escalation. Evaluate the exact immutable routing snapshot below. Return one JSON object and no prose. Probabilities are integer parts per million from 0 to 1000000. primary_probabilities must cover every available candidate plus `none` exactly once and sum to 1000000. contributions must cover every available candidate exactly once. primary_responder must be a maximum-probability alternative. Required keys: primary_responder, primary_probabilities, confidence, needs_collaboration, needs_clarification, contributions, high_impact.\n\n{}",
        serde_json::to_string_pretty(request)?
    ))
}

fn extract_json(reply: &str) -> &str {
    let trimmed = reply.trim();
    let without_open = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .trim();
    without_open
        .strip_suffix("```")
        .unwrap_or(without_open)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::extract_json;

    #[test]
    fn strict_json_extraction_accepts_plain_and_fenced_objects() {
        assert_eq!(extract_json(" {\"a\":1} "), "{\"a\":1}");
        assert_eq!(extract_json("```json\n{\"a\":1}\n```"), "{\"a\":1}");
    }
}
