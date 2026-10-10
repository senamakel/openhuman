use super::*;
use crate::config::schema::{RecoveryClassifier, RecoveryConfig};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tinytools_jev::recovery::*;

#[derive(Debug, Default)]
struct Fake {
    calls: AtomicUsize,
    fail: bool,
    pending: bool,
    low_confidence: bool,
    invalid: bool,
    changing_class: bool,
    force_class: Option<&'static str>,
    recoverability: Option<f64>,
}
#[async_trait::async_trait]
impl RecoveryEvaluator for Fake {
    async fn evaluate(
        &self,
        request: &RecoveryRequest,
    ) -> Result<RecoveryDecision, tinytools::RankError> {
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        let class = if let Some(class) = self.force_class {
            class
        } else if self.changing_class && index % 2 == 1 {
            "wrong_arguments"
        } else {
            "wrong_tool"
        };
        assert!(!request.observation.argument_shape.contains("secret-value"));
        if self.pending {
            std::future::pending::<()>().await;
        }
        if self.fail {
            return Err(tinytools::RankError::backend("failure"));
        }
        Ok(RecoveryDecision {
            answers: request
                .questions
                .iter()
                .map(|q| {
                    (
                        q.id().to_owned(),
                        match q {
                            RecoveryQuestion::Choice { options, .. } => RecoveryAnswer::Choice {
                                probabilities: options
                                    .iter()
                                    .map(|o| {
                                        (
                                            o.key.clone(),
                                            if (q.id() == "class" && o.key == class)
                                                || (q.id() == "alternate"
                                                    && o.key == "lookup_public")
                                            {
                                                1.0
                                            } else {
                                                0.0
                                            },
                                        )
                                    })
                                    .collect(),
                                confidence: if self.invalid {
                                    f64::NAN
                                } else if self.low_confidence {
                                    0.1
                                } else {
                                    1.0
                                },
                            },
                            RecoveryQuestion::Noul { .. } => {
                                RecoveryAnswer::Noul(self.recoverability.unwrap_or(1.0))
                            }
                            RecoveryQuestion::Score { rubric, .. } => RecoveryAnswer::Score {
                                probabilities: vec![1.0 / rubric.len() as f64; rubric.len()],
                                confidence: 1.0,
                            },
                        },
                    )
                })
                .collect(),
            input_tokens: None,
            output_tokens: None,
            latency: std::time::Duration::ZERO,
            attempts: 1,
        })
    }
}
fn configured(mode: RecoveryClassifier, fake: Arc<Fake>) -> RepeatedToolFailureMiddleware {
    let config = RecoveryConfig {
        classifier: mode,
        ..Default::default()
    };
    RepeatedToolFailureMiddleware::new(SteeringHandle::allow_all(), 3, Arc::new(Mutex::new(None)))
        .with_recovery_evaluator(config, fake)
        .with_tool_facts(Arc::new(|_, _| {
            Some(super::super::call_effect::ToolEffectFacts {
                classified: Some(super::super::call_effect::CallEffect::ReadOnly),
                external: false,
                elevated: false,
            })
        }))
}
async fn attempt(
    mw: &RepeatedToolFailureMiddleware,
    id: &str,
    error: Option<&str>,
) -> TaToolResult {
    let mut call = TaToolCall::new(
        id,
        "read_catalog",
        serde_json::json!({"query": "secret-value", "limit": id}),
    );
    let mut context = ctx();
    mw.before_tool(&mut context, &(), &mut call).await.unwrap();
    let mut result = match error {
        Some(e) => failing_result("read_catalog", e),
        None => TaToolResult::success("ok"),
    };
    mw.after_tool(
        &mut context,
        &(),
        &invocation(id, "read_catalog"),
        &mut result,
    )
    .await
    .unwrap();
    result
}
#[tokio::test]
async fn advisory_unknown_failure_queues_ephemeral_nudge_without_rewriting_result() {
    let fake = Arc::new(Fake::default());
    let mw = configured(RecoveryClassifier::Jev, fake.clone());
    let result = attempt(&mw, "one", Some("A capability mismatch occurred")).await;
    assert!(result.is_error);
    assert_eq!(tool_result_text(&result), "A capability mismatch occurred");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert!(mw
        .take_pending_nudges()
        .iter()
        .any(|n| n.contains("Advisory recovery")));
    assert!(mw.take_pending_nudges().is_empty());
}
#[tokio::test]
async fn success_keywords_hard_facts_and_executed_writes_never_evaluate() {
    let fake = Arc::new(Fake::default());
    let mw = configured(RecoveryClassifier::Jev, fake.clone());
    attempt(&mw, "ok", None).await;
    attempt(&mw, "policy", Some("403 Forbidden")).await;
    attempt(&mw, "schema", Some("schema validation failed")).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    let mw =
        configured(RecoveryClassifier::Jev, fake.clone()).with_tool_facts(Arc::new(|_, _| None));
    attempt(&mw, "write", Some("A capability mismatch occurred")).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn compare_failure_matches_keyword_nudges_and_outputs() {
    let fake = Arc::new(Fake::default());
    let compare = configured(RecoveryClassifier::Compare, fake.clone());
    let keyword = configured(RecoveryClassifier::Keywords, Arc::new(Fake::default()));
    for id in ["one", "two"] {
        let a = attempt(&compare, id, Some("A capability mismatch occurred")).await;
        let b = attempt(&keyword, id, Some("A capability mismatch occurred")).await;
        assert_eq!(tool_result_text(&a), tool_result_text(&b));
        assert_eq!(compare.take_pending_nudges(), keyword.take_pending_nudges());
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn provider_failure_and_varied_arguments_keep_stable_operation_ceiling() {
    let fake = Arc::new(Fake {
        fail: true,
        ..Default::default()
    });
    let handle = SteeringHandle::allow_all();
    // This fixture accesses the middleware's public steering seam via a
    // separate instance so a stable resource ceiling can be observed.
    let mw = RepeatedToolFailureMiddleware::new(handle.clone(), 8, Arc::new(Mutex::new(None)))
        .with_recovery_evaluator(
            RecoveryConfig {
                classifier: RecoveryClassifier::Jev,
                ..Default::default()
            },
            fake,
        )
        .with_tool_facts(Arc::new(|_, _| {
            Some(super::super::call_effect::ToolEffectFacts {
                classified: Some(super::super::call_effect::CallEffect::ReadOnly),
                external: false,
                elevated: false,
            })
        }));
    for id in ["one", "two", "three", "four", "five", "six"] {
        attempt(&mw, id, Some("A capability mismatch occurred")).await;
    }
    assert_eq!(drain_pause_count(&handle), 1);
}

#[tokio::test]
async fn cancelled_and_timed_out_advice_preserve_failure_and_fallback() {
    let fake = Arc::new(Fake {
        pending: true,
        ..Default::default()
    });
    let config = RecoveryConfig {
        classifier: RecoveryClassifier::Jev,
        decision_timeout_ms: 1,
        ..Default::default()
    };
    let mw = configured(RecoveryClassifier::Jev, fake.clone())
        .with_recovery_evaluator(config, fake.clone());
    let mut call = TaToolCall::new("cancel", "read_catalog", serde_json::json!({}));
    let mut context = ctx();
    context.cancellation.cancel();
    mw.before_tool(&mut context, &(), &mut call).await.unwrap();
    let mut result = failing_result("read_catalog", "capability mismatch");
    mw.after_tool(
        &mut context,
        &(),
        &invocation("cancel", "read_catalog"),
        &mut result,
    )
    .await
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    assert!(mw.take_pending_nudges().is_empty());
    attempt(&mw, "timeout", Some("capability mismatch")).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert!(mw.take_pending_nudges().is_empty());
}

#[tokio::test]
async fn no_credentials_or_disabled_policy_never_calls_provider() {
    let fake = Arc::new(Fake::default());
    for classifier in [RecoveryClassifier::Off, RecoveryClassifier::Keywords] {
        let mw = configured(classifier, fake.clone());
        attempt(&mw, "disabled", Some("capability mismatch")).await;
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn accepted_advice_varied_arguments_remain_bounded_across_unrelated_success() {
    let fake = Arc::new(Fake::default());
    let handle = SteeringHandle::allow_all();
    let mw = RepeatedToolFailureMiddleware::new(handle.clone(), 8, Arc::new(Mutex::new(None)))
        .with_recovery_evaluator(
            RecoveryConfig {
                classifier: RecoveryClassifier::Jev,
                ..Default::default()
            },
            fake.clone(),
        )
        .with_tool_facts(Arc::new(|_, _| {
            Some(super::super::call_effect::ToolEffectFacts {
                classified: Some(super::super::call_effect::CallEffect::ReadOnly),
                external: false,
                elevated: false,
            })
        }));
    attempt(&mw, "one", Some("capability mismatch")).await;
    let mut other = TaToolCall::new("other", "read_other", serde_json::json!({}));
    mw.before_tool(&mut ctx(), &(), &mut other).await.unwrap();
    let mut success = TaToolResult::success("done");
    mw.after_tool(
        &mut ctx(),
        &(),
        &invocation("other", "read_other"),
        &mut success,
    )
    .await
    .unwrap();
    attempt(&mw, "two", Some("different capability mismatch")).await;
    attempt(&mw, "three", Some("another capability mismatch")).await;
    assert_eq!(drain_pause_count(&handle), 1);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn advice_decision_ceiling_never_queues_additional_evaluations() {
    let fake = Arc::new(Fake::default());
    let mw = configured(RecoveryClassifier::Jev, fake.clone()).with_recovery_evaluator(
        RecoveryConfig {
            classifier: RecoveryClassifier::Jev,
            max_decisions_per_run: 1,
            ..Default::default()
        },
        fake.clone(),
    );
    attempt(&mw, "one", Some("capability mismatch")).await;
    attempt(&mw, "two", Some("different capability mismatch")).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn alternate_advice_requires_opt_in_and_counted_second_decision() {
    let fake = Arc::new(Fake::default());
    let config = RecoveryConfig {
        classifier: RecoveryClassifier::Jev,
        alternate_tools: true,
        ..Default::default()
    };
    let mw = configured(RecoveryClassifier::Jev, fake.clone())
        .with_recovery_evaluator(config, fake.clone());
    mw.set_recovery_candidates(vec![tinytools_jev::JevOption {
        key: "lookup_public".into(),
        description: "Look up public catalog entries".into(),
    }]);
    attempt(&mw, "alternate", Some("A capability mismatch occurred")).await;
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    assert!(mw
        .take_pending_nudges()
        .iter()
        .any(|n| n.contains("lookup_public")));
}

struct RecoveryTool {
    name: &'static str,
    hidden: bool,
    fail: bool,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl tinytools::Tool for RecoveryTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "Look up public catalog entries"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    fn policy(&self) -> tinytools::ToolPolicy {
        let mut policy = tinytools::ToolPolicy::read_only();
        match self.name {
            "scoped_lookup" => policy.access.workspace = tinytools::WorkspaceAccess::Scoped,
            "credential_lookup" => policy.access.credentials.push("account".into()),
            "approval_lookup" => policy.access.approval_required = true,
            "external_lookup" => policy.side_effects.external_service = true,
            "unclassified_lookup" => policy.classified = false,
            _ => {}
        }
        policy
    }
    fn permission_level(&self) -> tinytools::PermissionLevel {
        if self.name == "write_lookup" {
            tinytools::PermissionLevel::Write
        } else {
            tinytools::PermissionLevel::ReadOnly
        }
    }
    fn exposure(&self) -> tinytools::ToolExposure {
        if self.hidden {
            tinytools::ToolExposure::Hidden
        } else {
            tinytools::ToolExposure::Direct
        }
    }
    async fn execute(&self, _: serde_json::Value) -> anyhow::Result<TaToolResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(if self.fail {
            TaToolResult::error("A capability mismatch occurred")
        } else {
            TaToolResult::success("public result")
        })
    }
}

#[tokio::test]
async fn main_model_alternate_call_still_passes_normal_admission_and_hidden_calls_never_execute() {
    for permit_alternate in [true, false] {
        use tinyagents_harness::middleware::ToolAllowlistMiddleware;
        use tinyagents_harness::runtime::AgentHarness;
        use tinyagents_harness::testkit::ScriptedModel;
        use tinyinference_llm::model::ModelResponse;
        let first = Arc::new(RecoveryTool {
            name: "read_catalog",
            hidden: false,
            fail: true,
            calls: AtomicUsize::new(0),
        });
        let alternate = Arc::new(RecoveryTool {
            name: "lookup_public",
            hidden: false,
            fail: false,
            calls: AtomicUsize::new(0),
        });
        let hidden = Arc::new(RecoveryTool {
            name: "hidden_lookup",
            hidden: true,
            fail: false,
            calls: AtomicUsize::new(0),
        });
        let denied = Arc::new(RecoveryTool {
            name: "denied_lookup",
            hidden: false,
            fail: false,
            calls: AtomicUsize::new(0),
        });
        let fake = Arc::new(Fake::default());
        let mw = Arc::new(
            configured(RecoveryClassifier::Jev, fake.clone()).with_recovery_evaluator(
                RecoveryConfig {
                    classifier: RecoveryClassifier::Jev,
                    alternate_tools: true,
                    ..Default::default()
                },
                fake.clone(),
            ),
        );
        let response = |id, name| {
            let mut r = ModelResponse::assistant("");
            r.message.tool_calls = vec![TaToolCall::new(id, name, serde_json::json!({}))];
            r.finish_reason = Some("tool_calls".into());
            r
        };
        let model = Arc::new(ScriptedModel::new(vec![
            response("first", "read_catalog"),
            response("next", "lookup_public"),
            response("denied", "denied_lookup"),
            response("hidden", "hidden_lookup"),
            ModelResponse::assistant("done"),
        ]));
        let mut harness: AgentHarness<(), crate::agent::tinyagents::host::OpenHumanRunContext> =
            AgentHarness::new();
        harness.register_model("mock", model.clone());
        for tool in [&first, &alternate, &denied, &hidden] {
            harness.register_tool(tool.clone());
        }
        let session = crate::tools::agent_policy::ToolPolicyEngine::build_session_from_refs(
            "test",
            "test",
            "test",
            &Default::default(),
            &[first.as_ref(), alternate.as_ref(), hidden.as_ref()],
            &Default::default(),
        );
        let candidates =
            super::super::recovery_advice::admitted_candidates(harness.tools(), Some(&session));
        assert!(!candidates.iter().any(|c| c.key == "hidden_lookup"));
        assert!(
            !candidates.iter().any(|c| c.key == "denied_lookup"),
            "session-unknown tools default deny"
        );
        mw.set_recovery_candidates(candidates);
        harness.push_middleware(mw.clone());
        harness.push_middleware(Arc::new(ToolAllowlistMiddleware::new(
            if permit_alternate {
                vec!["read_catalog", "lookup_public"]
            } else {
                vec!["read_catalog"]
            },
        )));
        harness.push_middleware(Arc::new(mw.nudge_injector()));
        let run = harness
            .invoke_in_context(
                &(),
                ctx(),
                vec![TaMessage::user("look up the public catalog")],
            )
            .await
            .unwrap();
        assert_eq!(run.text().as_deref(), Some("done"));
        assert_eq!(first.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            alternate.calls.load(Ordering::SeqCst),
            usize::from(permit_alternate)
        );
        assert_eq!(denied.calls.load(Ordering::SeqCst), 0);
        assert_eq!(hidden.calls.load(Ordering::SeqCst), 0);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
        assert!(model.requests()[1]
            .messages
            .iter()
            .any(|m| m.text().contains("lookup_public")));
        assert!(!run
            .messages
            .iter()
            .any(|m| m.text().contains("Advisory recovery")));
    }
}

#[tokio::test]
async fn low_confidence_and_malformed_decisions_keep_keyword_behavior() {
    for fake in [
        Fake {
            low_confidence: true,
            ..Default::default()
        },
        Fake {
            invalid: true,
            ..Default::default()
        },
    ] {
        let fake = Arc::new(fake);
        let mw = configured(RecoveryClassifier::Jev, fake.clone());
        let keywords = configured(RecoveryClassifier::Keywords, Arc::new(Fake::default()));
        for id in ["one", "two"] {
            let result = attempt(&mw, id, Some("A capability mismatch occurred")).await;
            let baseline = attempt(&keywords, id, Some("A capability mismatch occurred")).await;
            assert_eq!(tool_result_text(&result), tool_result_text(&baseline));
            assert_eq!(mw.take_pending_nudges(), keywords.take_pending_nudges());
        }
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn classification_disagreement_does_not_reset_stable_failure_allowance() {
    let handle = SteeringHandle::allow_all();
    let fake = Arc::new(Fake {
        changing_class: true,
        ..Default::default()
    });
    let mw = RepeatedToolFailureMiddleware::new(handle.clone(), 10, Arc::new(Mutex::new(None)))
        .with_recovery_evaluator(
            RecoveryConfig {
                classifier: RecoveryClassifier::Jev,
                ..Default::default()
            },
            fake.clone(),
        )
        .with_tool_facts(Arc::new(|_, _| {
            Some(super::super::call_effect::ToolEffectFacts {
                classified: Some(super::super::call_effect::CallEffect::ReadOnly),
                external: false,
                elevated: false,
            })
        }));
    for id in ["one", "two", "three"] {
        attempt(&mw, id, Some("A capability mismatch occurred")).await;
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
    assert_eq!(drain_pause_count(&handle), 1);
}

#[tokio::test]
async fn unavailable_recoverability_controls_permanent_halt_projection() {
    for value in [0.0, 1.0] {
        let handle = SteeringHandle::allow_all();
        let fake = Arc::new(Fake {
            force_class: Some("unavailable"),
            recoverability: Some(value),
            ..Default::default()
        });
        let mw = RepeatedToolFailureMiddleware::new(handle.clone(), 10, Arc::new(Mutex::new(None)))
            .with_recovery_evaluator(
                RecoveryConfig {
                    classifier: RecoveryClassifier::Jev,
                    ..Default::default()
                },
                fake,
            )
            .with_tool_facts(Arc::new(|_, _| {
                Some(super::super::call_effect::ToolEffectFacts {
                    classified: Some(super::super::call_effect::CallEffect::ReadOnly),
                    external: false,
                    elevated: false,
                })
            }));
        attempt(&mw, "one", Some("A capability mismatch occurred")).await;
        assert_eq!(drain_pause_count(&handle), usize::from(value == 0.0));
    }
}
#[tokio::test]
async fn wrong_arguments_without_recoverability_does_not_encourage_a_corrected_retry() {
    for value in [0.0, 1.0] {
        let fake = Arc::new(Fake {
            force_class: Some("wrong_arguments"),
            recoverability: Some(value),
            ..Default::default()
        });
        let mw = configured(RecoveryClassifier::Jev, fake);
        attempt(&mw, "one", Some("A capability mismatch occurred")).await;
        assert_eq!(
            mw.take_pending_nudges()
                .iter()
                .any(|n| n.contains("choose a corrected call")),
            value == 1.0
        );
    }
}

#[test]
fn candidate_snapshot_excludes_scope_credentials_approval_external_and_unclassified_tools() {
    let mut harness: tinyagents_harness::runtime::AgentHarness<
        (),
        crate::agent::tinyagents::host::OpenHumanRunContext,
    > = tinyagents_harness::runtime::AgentHarness::new();
    for name in [
        "lookup_public",
        "scoped_lookup",
        "credential_lookup",
        "approval_lookup",
        "external_lookup",
        "unclassified_lookup",
        "write_lookup",
    ] {
        harness.register_tool(Arc::new(RecoveryTool {
            name,
            hidden: false,
            fail: false,
            calls: AtomicUsize::new(0),
        }));
    }
    let candidates = super::super::recovery_advice::admitted_candidates(harness.tools(), None);
    assert_eq!(
        candidates
            .iter()
            .map(|c| c.key.as_str())
            .collect::<Vec<_>>(),
        vec!["lookup_public"]
    );
}

#[tokio::test]
async fn compare_evaluates_opt_in_alternates_without_projecting_advice() {
    let fake = Arc::new(Fake::default());
    let mw = configured(RecoveryClassifier::Compare, fake.clone()).with_recovery_evaluator(
        RecoveryConfig {
            classifier: RecoveryClassifier::Compare,
            alternate_tools: true,
            ..Default::default()
        },
        fake.clone(),
    );
    mw.set_recovery_candidates(vec![tinytools_jev::JevOption {
        key: "lookup_public".into(),
        description: "Look up public catalog entries".into(),
    }]);
    let keywords = configured(RecoveryClassifier::Keywords, Arc::new(Fake::default()));
    let result = attempt(&mw, "one", Some("A capability mismatch occurred")).await;
    let baseline = attempt(&keywords, "one", Some("A capability mismatch occurred")).await;
    assert_eq!(tool_result_text(&result), tool_result_text(&baseline));
    assert_eq!(mw.take_pending_nudges(), keywords.take_pending_nudges());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
}
