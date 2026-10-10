//! Mechanical recovery question transport. Wording and validation are TinyTools-owned.
use openhuman_embed::__host::config::Config;
use openhuman_embed::recovery::RecoveryProviderFactory;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tinyjevclient::{Answer, Choice, Client, EvaluationRequest, Noul, Question, Score};
use tinytools::RankError;
use tinytools_jev::recovery::{
    RecoveryAnswer, RecoveryDecision, RecoveryEvaluator, RecoveryQuestion, RecoveryRequest,
};

/// Transports TinyTools recovery questions through a preconfigured Jev client.
#[derive(Debug)]
pub struct TinyJevRecoveryEvaluator {
    client: Client,
    deadline: Duration,
}
impl TinyJevRecoveryEvaluator {
    /// Use the supplied credential snapshot and bound each evaluation by `deadline`.
    pub fn new(client: Client, deadline: Duration) -> Self {
        Self { client, deadline }
    }
    fn build(request: &RecoveryRequest) -> EvaluationRequest {
        let o = &request.observation;
        EvaluationRequest {
            model: "jev-latest".into(),
            state: json!({"version": request.version, "tool": o.tool_description,
                "diagnostic": o.failure, "argument_shape": o.argument_shape,
                "phase": format!("{:?}", o.phase), "effect": format!("{:?}", o.effect),
                "attempts": o.attempts, "repeated_failures": o.repeated_failures,
                "concrete_correction": o.concrete_correction}),
            questions: request
                .questions
                .iter()
                .map(|q| {
                    let wire = match q {
                        RecoveryQuestion::Choice {
                            prompt, options, ..
                        } => Question::Choice(Choice {
                            instructions: json!(prompt),
                            criteria: options
                                .iter()
                                .map(|o| (o.key.clone(), Some(json!(o.description))))
                                .collect(),
                        }),
                        RecoveryQuestion::Noul { prompt, .. } => Question::Noul(Noul {
                            instructions: json!(prompt),
                            criteria: None,
                        }),
                        RecoveryQuestion::Score { prompt, rubric, .. } => Question::Score(Score {
                            instructions: json!(prompt),
                            criteria: rubric.iter().map(|r| json!(r)).collect(),
                        }),
                    };
                    (q.id().to_owned(), wire)
                })
                .collect(),
        }
    }
}
#[async_trait::async_trait]
impl RecoveryEvaluator for TinyJevRecoveryEvaluator {
    async fn evaluate(&self, request: &RecoveryRequest) -> Result<RecoveryDecision, RankError> {
        let result =
            tokio::time::timeout(self.deadline, self.client.evaluate(&Self::build(request)))
                .await
                .map_err(|_| RankError::Timeout)?
                .map_err(|_| RankError::backend("recovery provider unavailable"))?;
        let mut answers = BTreeMap::new();
        // Preserve exact IDs, types and complete distributions; TinyTools rejects
        // missing/extra/type-mismatched answers. Client validates Score legends.
        for (id, answer) in result.response.answers {
            let mapped = match answer {
                Answer::Choice(c) => RecoveryAnswer::Choice {
                    probabilities: c.probabilities,
                    confidence: c.confidence,
                },
                Answer::Noul(n) => RecoveryAnswer::Noul(n.noul),
                Answer::Score(s) => {
                    let n = s.probabilities.len();
                    let probabilities = (0..n)
                        .map(|i| s.probabilities.get(&i.to_string()).copied())
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| RankError::backend("invalid recovery score"))?;
                    RecoveryAnswer::Score {
                        probabilities,
                        confidence: s.confidence,
                    }
                }
            };
            answers.insert(id, mapped);
        }
        Ok(RecoveryDecision {
            answers,
            input_tokens: result.response.usage.input_tokens,
            output_tokens: result.response.usage.output_tokens,
            latency: result.latency,
            attempts: result.attempts,
        })
    }
}

fn resolved(
    config: &Config,
    env: super::route::EnvLookup<'_>,
) -> Result<super::route::ResolvedRoute, RankError> {
    config
        .agent
        .recovery
        .validate()
        .map_err(RankError::invalid_input)?;
    let mut config = config.clone();
    if let Some(route) = config.agent.recovery.jev_route.clone() {
        config.agent.tool_search.jev_route = route;
    }
    // Validate inherited routes too: recovery must never silently change route.
    if !matches!(
        config.agent.tool_search.jev_route.as_str(),
        "auto" | "tinyhumans" | "typesafe" | "openrouter"
    ) {
        return Err(RankError::invalid_input("invalid inherited recovery route"));
    }
    if let Some(base) = config.agent.recovery.jev_base_url.clone() {
        config.agent.tool_search.jev_base_url = Some(base);
    }
    super::route::resolve(&config, env)
}

/// Freeze credential and URL at turn assembly, with a single transport attempt.
/// Signed-out or invalid configuration leaves the core's normal fallback intact.
pub fn recovery_provider() -> RecoveryProviderFactory {
    Arc::new(|config| {
        let mut route = resolved(config, &super::route::process_env).ok()?;
        route.client.retry.max_retries = 0;
        let client = Client::new(route.client).ok()?;
        Some(Arc::new(TinyJevRecoveryEvaluator::new(
            client,
            Duration::from_millis(config.agent.recovery.decision_timeout_ms),
        )))
    })
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
