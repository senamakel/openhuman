//! Route one company-style request through Jev, then run the selected OpenHuman seats.
//!
//! See `README.md` beside this file for required environment and evidence
//! boundaries. This is a paid live example and is never run by tests.

mod reasoning;
mod transport;

use std::sync::Arc;
use std::time::Instant;

use openhuman_core::agent::progress::AgentProgress;
use openhuman_core::config::Config;
use openhuman_core::core::runtime::{AGENT_WORKER_STACK_BYTES, MAX_BLOCKING_THREADS};
use openhuman_embed::{Access, Harness, Provider, Workspace};
use serde::Serialize;
use tinyhivemind_embed::{
    route_message, ConversationKind, ConversationRef, RouteCandidate, RoutingPlan, RoutingPolicy,
    RoutingRequest,
};
use tinyhivemind_typesafe::JevRouter;
use uuid::Uuid;

use reasoning::{OpenHumanReasoningRouter, ReasoningTrace};
use transport::{JevTransport, SystemOneTrace};

const MAX_LIVE_SEATS: usize = 25;

#[derive(Debug, Serialize)]
struct SeatRecord {
    agent_id: String,
    session_id: String,
    reply: String,
    latency_ms: u64,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cost_usd: f64,
}

#[derive(Debug, Serialize)]
struct ProofTotals {
    jev_input_tokens: u64,
    jev_output_tokens: u64,
    openhuman_input_tokens: u64,
    openhuman_output_tokens: u64,
    openhuman_cost_usd: f64,
    reasoning_input_tokens: u64,
    reasoning_output_tokens: u64,
    reasoning_cost_usd: f64,
}

#[derive(Debug, Serialize)]
struct ProofRecord {
    content_included: bool,
    proof_id: String,
    message: String,
    candidate_snapshot: Vec<RouteCandidate>,
    jev_calls: Vec<SystemOneTrace>,
    reasoning_calls: Vec<ReasoningTrace>,
    accepted_plan: RoutingPlan,
    turns: Vec<SeatRecord>,
    totals: ProofTotals,
}

#[derive(Clone, Copy, Debug, Default)]
struct TurnCost {
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    cost_usd: f64,
}

/// Build the tuned runtime required by the OpenHuman harness and run the proof.
fn main() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(false).try_init();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(AGENT_WORKER_STACK_BYTES)
        .max_blocking_threads(MAX_BLOCKING_THREADS)
        .build()?;
    runtime.block_on(run())
}

/// Route the requested decision, run only the accepted seats, and print evidence.
async fn run() -> anyhow::Result<()> {
    let message = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("pass the exact company request as the first argument"))?;
    let policy: RoutingPolicy = serde_json::from_str(
        &std::env::var("JEV_ROUTING_POLICY_JSON")
            .map_err(|_| anyhow::anyhow!("JEV_ROUTING_POLICY_JSON is required"))?,
    )?;
    if policy.round_width == 0 || policy.round_width > MAX_LIVE_SEATS {
        anyhow::bail!("routing policy round_width must be in 1..={MAX_LIVE_SEATS}");
    }

    let candidates = candidates();
    let request = RoutingRequest {
        message: message.clone(),
        conversation: ConversationRef {
            id: "openhuman-live-proof".into(),
            kind: ConversationKind::Desk,
            thread_root: None,
        },
        desk_purpose: Some(
            "Evaluate a company decision with implementation, financial, research, operational, legal, and executive judgment".into(),
        ),
        thread_context: Vec::new(),
        candidates: candidates.clone(),
        roster_version: 1,
        policy,
    };

    let harness = Arc::new(build_harness().await?);
    let transport = JevTransport::from_env()?;
    let router = JevRouter::new(transport.clone());
    let reasoning = OpenHumanReasoningRouter::new(Arc::clone(&harness));
    let accepted_plan = route_message(
        Some(&router),
        Some(&reasoning),
        &request,
        None,
        &candidates[0].id,
    )
    .await;

    let selected = selected_agents(&accepted_plan)?;
    let selected_candidates = resolve_candidates(&selected, &candidates)?;
    let proof_id = Uuid::new_v4().to_string();
    let mut running = tokio::task::JoinSet::new();
    for candidate in selected_candidates {
        let harness = Arc::clone(&harness);
        let message = message.clone();
        let proof_id = proof_id.clone();
        running.spawn(async move { run_seat(harness, candidate, message, proof_id).await });
    }

    let mut turns = Vec::new();
    while let Some(result) = running.join_next().await {
        turns.push(result??);
    }
    turns.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));

    let jev_calls = transport.traces().map_err(anyhow::Error::msg)?;
    let reasoning_calls = reasoning.traces().map_err(anyhow::Error::msg)?;
    let totals = totals(&jev_calls, &reasoning_calls, &turns);
    let mut record = ProofRecord {
        content_included: include_content(),
        proof_id,
        message,
        candidate_snapshot: candidates,
        jev_calls,
        reasoning_calls,
        accepted_plan,
        turns,
        totals,
    };
    if !record.content_included {
        redact_content(&mut record);
    }
    println!("{}", serde_json::to_string_pretty(&record)?);
    Ok(())
}

/// Validate every selected id before any paid seat task is spawned.
fn resolve_candidates(
    selected: &[String],
    candidates: &[RouteCandidate],
) -> anyhow::Result<Vec<RouteCandidate>> {
    selected
        .iter()
        .map(|agent_id| {
            candidates
                .iter()
                .find(|candidate| candidate.id == *agent_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("accepted plan named unknown agent {agent_id}"))
        })
        .collect()
}

/// Return whether the operator explicitly opted into content-bearing stdout.
fn include_content() -> bool {
    std::env::var("JEV_PROOF_INCLUDE_CONTENT").is_ok_and(|value| value == "1")
}

/// Remove user/model text while preserving auditable routing metadata.
fn redact_content(record: &mut ProofRecord) {
    const REDACTED: &str = "[redacted; set JEV_PROOF_INCLUDE_CONTENT=1 for secure output]";
    record.message = REDACTED.into();
    for call in &mut record.jev_calls {
        if let Some(state) = call.request.get_mut("state") {
            if let Some(message) = state.get_mut("message") {
                *message = serde_json::Value::String(REDACTED.into());
            }
            if let Some(context) = state.get_mut("thread_context") {
                *context = serde_json::Value::Array(Vec::new());
            }
        }
    }
    for call in &mut record.reasoning_calls {
        call.prompt = REDACTED.into();
        call.reply = REDACTED.into();
    }
    for turn in &mut record.turns {
        turn.reply = REDACTED.into();
    }
}

/// Return the fixed OpenCompany-style candidate snapshot used by this proof.
fn candidates() -> Vec<RouteCandidate> {
    [
        (
            "engineering",
            "Engineering",
            "software architecture and implementation",
            &["rust", "systems design"][..],
        ),
        (
            "finance",
            "Finance",
            "financial modeling, pricing, and unit economics",
            &["forecasting", "cost analysis"],
        ),
        (
            "research",
            "Research",
            "source evaluation and market research",
            &["evidence synthesis", "fact checking"],
        ),
        (
            "operations",
            "Operations",
            "delivery planning and operational risk",
            &["rollouts", "incident response"],
        ),
        (
            "legal",
            "Legal and Compliance",
            "contracts, regulation, and compliance risk",
            &["legal review", "policy"],
        ),
        (
            "executive",
            "Executive",
            "cross-functional tradeoffs and final recommendations",
            &["strategy", "decision making"],
        ),
        (
            "verification",
            "Verification",
            "independent checking and adversarial review",
            &["validation", "contradiction detection"],
        ),
    ]
    .into_iter()
    .map(|(id, label, description, capabilities)| RouteCandidate {
        id: id.into(),
        label: label.into(),
        role: Some(label.into()),
        description: Some(description.into()),
        capabilities: capabilities.iter().map(|value| (*value).into()).collect(),
        learned_topics: Vec::new(),
        available: true,
    })
    .collect()
}

/// Extract one bounded, duplicate-free set of seats from an accepted plan.
fn selected_agents(plan: &RoutingPlan) -> anyhow::Result<Vec<String>> {
    let selected = match plan {
        RoutingPlan::One { responder_id, .. } => vec![responder_id.clone()],
        RoutingPlan::Hive {
            primary_id,
            invited_ids,
            ..
        } => std::iter::once(primary_id.clone())
            .chain(invited_ids.iter().cloned())
            .collect(),
        RoutingPlan::Clarify { .. } => Vec::new(),
        RoutingPlan::Fallback { responder_id, .. } => vec![responder_id.clone()],
    };
    if selected.len() > MAX_LIVE_SEATS {
        anyhow::bail!(
            "accepted routing plan selected {} seats, above the live limit of {MAX_LIVE_SEATS}",
            selected.len()
        );
    }
    let unique: std::collections::BTreeSet<_> = selected.iter().collect();
    if unique.len() != selected.len() {
        anyhow::bail!("accepted routing plan contains duplicate seat ids");
    }
    Ok(selected)
}

/// Build one ephemeral, read-only Harness against the explicitly named provider.
async fn build_harness() -> anyhow::Result<Harness> {
    let base_url = std::env::var("OPENHUMAN_EXAMPLE_BASE_URL")
        .map_err(|_| anyhow::anyhow!("OPENHUMAN_EXAMPLE_BASE_URL is required"))?;
    let api_key = std::env::var("OPENHUMAN_EXAMPLE_API_KEY")
        .map_err(|_| anyhow::anyhow!("OPENHUMAN_EXAMPLE_API_KEY is required"))?;
    let model = std::env::var("OPENHUMAN_EXAMPLE_MODEL")
        .map_err(|_| anyhow::anyhow!("OPENHUMAN_EXAMPLE_MODEL is required"))?;
    let mut config = Config::default();
    config.local_ai.runtime_enabled = false;
    config.runtime_python.enabled = false;
    config.memory_tree.spacy_enabled = false;
    config.memory_tree.embedding_endpoint = None;
    config.memory_tree.embedding_model = None;
    config.memory_tree.embedding_strict = false;
    config.default_temperature = 0.0;
    Harness::builder()
        .config(config)
        .workspace(Workspace::Ephemeral)
        .provider(Provider::openai_compatible(base_url, api_key).model(model))
        .access(Access::readonly())
        .build()
        .await
        .map_err(Into::into)
}

/// Run one selected specialist in its stable company-and-agent session.
async fn run_seat(
    harness: Arc<Harness>,
    candidate: RouteCandidate,
    message: String,
    proof_id: String,
) -> anyhow::Result<SeatRecord> {
    let session_id = format!("openhuman-live-proof:{proof_id}:{}", candidate.id);
    let prompt = format!(
        "Company request:\n{message}\n\nYour assigned role is {}. Give a concise, evidence-aware recommendation from that role. State uncertainties and do not take actions.",
        candidate.description.as_deref().unwrap_or(&candidate.label)
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let collector = tokio::spawn(async move {
        let mut cost = TurnCost::default();
        while let Some(progress) = rx.recv().await {
            if let AgentProgress::TurnCostUpdated {
                input_tokens,
                output_tokens,
                cached_input_tokens,
                total_usd,
                ..
            } = progress
            {
                cost = TurnCost {
                    input_tokens,
                    output_tokens,
                    cached_input_tokens,
                    cost_usd: total_usd,
                };
            }
        }
        cost
    });
    let started = Instant::now();
    let outcome = harness
        .turn(prompt)
        .session(&session_id)
        .on_progress(tx)
        .send()
        .await?;
    let cost = tokio::time::timeout(std::time::Duration::from_secs(10), collector)
        .await
        .map_err(|_| anyhow::anyhow!("progress collector did not settle"))??;
    Ok(SeatRecord {
        agent_id: candidate.id,
        session_id: outcome.session_id,
        reply: outcome.reply,
        latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        input_tokens: cost.input_tokens,
        output_tokens: cost.output_tokens,
        cached_input_tokens: cost.cached_input_tokens,
        cost_usd: cost.cost_usd,
    })
}

/// Sum Jev, reasoning-escalation, and selected-seat metering independently.
fn totals(
    jev_calls: &[SystemOneTrace],
    reasoning_calls: &[ReasoningTrace],
    turns: &[SeatRecord],
) -> ProofTotals {
    let jev_input_tokens = jev_calls
        .iter()
        .filter_map(|trace| trace.response["usage"]["input_tokens"].as_u64())
        .sum();
    let jev_output_tokens = jev_calls
        .iter()
        .filter_map(|trace| trace.response["usage"]["output_tokens"].as_u64())
        .sum();
    ProofTotals {
        jev_input_tokens,
        jev_output_tokens,
        openhuman_input_tokens: turns.iter().map(|turn| turn.input_tokens).sum(),
        openhuman_output_tokens: turns.iter().map(|turn| turn.output_tokens).sum(),
        openhuman_cost_usd: turns.iter().map(|turn| turn.cost_usd).sum(),
        reasoning_input_tokens: reasoning_calls.iter().map(|turn| turn.input_tokens).sum(),
        reasoning_output_tokens: reasoning_calls.iter().map(|turn| turn.output_tokens).sum(),
        reasoning_cost_usd: reasoning_calls.iter().map(|turn| turn.cost_usd).sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::{candidates, resolve_candidates, selected_agents, MAX_LIVE_SEATS};
    use tinyhivemind_embed::{RoutingFallback, RoutingPlan};

    #[test]
    fn candidate_ids_are_unique_and_fallback_is_one_bounded_seat() {
        let candidates = candidates();
        let ids: std::collections::BTreeSet<_> =
            candidates.iter().map(|candidate| &candidate.id).collect();
        assert_eq!(ids.len(), candidates.len());
        assert_eq!(
            selected_agents(&RoutingPlan::Fallback {
                responder_id: "engineering".into(),
                reason: RoutingFallback::ProviderUnavailable,
            })
            .expect("fallback is bounded"),
            ["engineering"]
        );
    }

    #[test]
    fn duplicate_or_overwide_hive_plans_fail_closed() {
        use tinyhivemind::responder::Probability;
        use tinyhivemind_embed::{
            CandidateProbability, ContributionProbability, EvaluationDisposition, RoutingEvaluation,
        };

        let evaluation = RoutingEvaluation {
            primary_responder: "engineering".into(),
            primary_probabilities: vec![CandidateProbability {
                candidate_id: "engineering".into(),
                probability: Probability::ONE,
            }],
            confidence: Probability::ONE,
            needs_collaboration: Probability::ONE,
            needs_clarification: Probability::ZERO,
            contributions: vec![ContributionProbability {
                candidate_id: "engineering".into(),
                probability: Probability::ONE,
            }],
            high_impact: Probability::ZERO,
            model_identity: "test".into(),
            question_schema_version: 1,
            roster_version: 1,
            disposition: EvaluationDisposition::Accepted,
        };
        let duplicated = RoutingPlan::Hive {
            primary_id: "engineering".into(),
            invited_ids: vec!["engineering".into()],
            evaluation: evaluation.clone(),
        };
        assert!(selected_agents(&duplicated).is_err());

        let overwide = RoutingPlan::Hive {
            primary_id: "engineering".into(),
            invited_ids: (0..MAX_LIVE_SEATS)
                .map(|index| format!("seat-{index}"))
                .collect(),
            evaluation,
        };
        assert!(selected_agents(&overwide).is_err());
    }

    #[test]
    fn every_selected_id_is_validated_before_spawning() {
        let candidates = candidates();
        let selected = vec!["engineering".into(), "unknown".into()];
        assert!(resolve_candidates(&selected, &candidates).is_err());
    }
}
