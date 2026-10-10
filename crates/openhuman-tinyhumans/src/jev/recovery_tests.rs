use super::*;
use tinytools_jev::recovery::{RecoveryObservation, RecoveryPhase};

#[test]
fn recovery_transport_keeps_tinytools_questions_and_minimal_state() {
    let request = RecoveryRequest::new(RecoveryObservation::new(
        RecoveryPhase::Execution,
        "read catalog",
        "unrecognized diagnostic",
    ))
    .unwrap();
    let wire = TinyJevRecoveryEvaluator::build(&request);
    assert_eq!(wire.questions.len(), request.questions.len());
    assert_eq!(wire.state["diagnostic"], "unrecognized diagnostic");
    assert!(wire.state.get("transcript").is_none());
    for q in &request.questions {
        assert!(wire.questions.contains_key(q.id()));
    }
}

#[test]
fn recovery_route_inherits_and_explicit_typo_fails_closed() {
    let mut config = Config::default();
    config.agent.tool_search.jev_route = "typesafe".into();
    config.agent.tool_search.jev_base_url = Some("http://127.0.0.1:18080".into());
    let env = |_: &str| Some("test-key".to_owned());
    let route = resolved(&config, &env).unwrap();
    assert_eq!(route.client.base_url, "http://127.0.0.1:18080");
    config.agent.recovery.jev_route = Some("typo".into());
    assert!(resolved(&config, &env).is_err());
    config.agent.recovery.jev_route = Some("openrouter".into());
    config.agent.recovery.jev_base_url = Some("http://127.0.0.1:18081".into());
    assert_eq!(
        resolved(&config, &env).unwrap().client.base_url,
        "http://127.0.0.1:18081"
    );
}

#[tokio::test]
async fn local_transport_maps_complete_answers_and_rejects_malformed_answers() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let mut observation = RecoveryObservation::new(
        RecoveryPhase::Unknown,
        "read catalog",
        "capability mismatch",
    );
    observation.concrete_correction = Some("Use the documented parameter name".into());
    let request = RecoveryRequest::new(observation).unwrap();
    let answers: BTreeMap<_, _> = request.questions.iter().map(|q| (q.id().to_owned(), match q {
        RecoveryQuestion::Choice { options, .. } => json!({"type":"choice", "choice":"wrong_tool", "confidence":1.0,
            "probabilities":options.iter().map(|o| (o.key.clone(), if o.key == "wrong_tool" { 1.0 } else { 0.0 })).collect::<BTreeMap<_,_>>() }),
        RecoveryQuestion::Noul { .. } => json!({"type":"noul", "noul":1.0}),
        RecoveryQuestion::Score { rubric, .. } => json!({"type":"score", "score":1.1, "confidence":0.5,
            "probabilities":{"0":0.2,"1":0.5,"2":0.3},
            "legend":rubric.iter().enumerate().map(|(i,r)| (i.to_string(), json!(r))).collect::<BTreeMap<_,_>>() }),
    })).collect();
    Mock::given(method("POST")).and(path("/v1/systemone")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"model":"jev-latest", "answers":answers, "usage":{"input_tokens":12,"output_tokens":3}}))).mount(&server).await;
    let mut config = tinyjevclient::ClientConfig::new("test-key");
    config.base_url = server.uri();
    config.retry.max_retries = 0;
    let evaluator =
        TinyJevRecoveryEvaluator::new(Client::new(config.clone()).unwrap(), Duration::from_secs(1));
    let decision = evaluator.evaluate(&request).await.unwrap();
    assert_eq!(decision.answers.len(), request.questions.len());
    assert_eq!(decision.input_tokens, Some(12));
    assert_eq!(decision.output_tokens, Some(3));
    assert_eq!(decision.attempts, 1);
    let score_id = request
        .questions
        .iter()
        .find(|q| matches!(q, RecoveryQuestion::Score { .. }))
        .unwrap()
        .id();
    assert_eq!(
        decision.answers[score_id],
        RecoveryAnswer::Score {
            probabilities: vec![0.2, 0.5, 0.3],
            confidence: 0.5
        }
    );
    server.reset().await;
    let mut malformed = answers.clone();
    malformed.get_mut(score_id).unwrap()["probabilities"] = json!({"0":0.2,"1":0.5,"extra":0.3});
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"model":"jev-latest", "answers":malformed, "usage":{}})),
        )
        .mount(&server)
        .await;
    assert!(evaluator.evaluate(&request).await.is_err());
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"model":"jev-latest", "answers":{}, "usage":{}})),
        )
        .mount(&server)
        .await;
    // Typed client or generic adviser rejects missing answers; no host guesses.
    let result = evaluator.evaluate(&request).await;
    if let Ok(decision) = result {
        assert!(decision.answers.is_empty());
    }
    let adviser = tinytools_jev::recovery::RecoveryAdviser::new(
        Arc::new(TinyJevRecoveryEvaluator::new(
            Client::new(config.clone()).unwrap(),
            Duration::from_secs(1),
        )),
        Config::default().agent.recovery.thresholds(),
    )
    .unwrap();
    assert!(matches!(
        adviser.advise(request.observation.clone()).await.unwrap(),
        tinytools_jev::recovery::RecoveryAdvice::Abstained(_)
    ));
    server.reset().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"model":"jev-latest", "answers":{"class":{"type":"noul","noul":12.0}}, "usage":{}}))).mount(&server).await;
    assert!(evaluator.evaluate(&request).await.is_err());
}

#[tokio::test]
async fn transport_deadline_is_bounded_and_no_retry_is_started() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_delay(Duration::from_secs(1)))
        .mount(&server)
        .await;
    let mut config = tinyjevclient::ClientConfig::new("test-key");
    config.base_url = server.uri();
    config.retry.max_retries = 0;
    let evaluator =
        TinyJevRecoveryEvaluator::new(Client::new(config).unwrap(), Duration::from_millis(100));
    let request = RecoveryRequest::new(RecoveryObservation::new(
        RecoveryPhase::Unknown,
        "read catalog",
        "failure",
    ))
    .unwrap();
    assert!(matches!(
        evaluator.evaluate(&request).await,
        Err(RankError::Timeout)
    ));
    assert!(server.received_requests().await.unwrap().len() <= 1);
}

#[test]
fn score_question_preserves_library_rubric_order_without_interpreting_scores() {
    let mut observation =
        RecoveryObservation::new(RecoveryPhase::PreExecution, "read catalog", "bad schema");
    observation.concrete_correction = Some("Use the documented parameter name".into());
    let request = RecoveryRequest::new(observation).unwrap();
    let wire = TinyJevRecoveryEvaluator::build(&request);
    let score = request
        .questions
        .iter()
        .find_map(|q| {
            if let RecoveryQuestion::Score { id, rubric, .. } = q {
                Some((id, rubric))
            } else {
                None
            }
        })
        .unwrap();
    let Some(Question::Score(mapped)) = wire.questions.get(score.0) else {
        panic!("missing score")
    };
    assert_eq!(
        mapped.criteria,
        score.1.iter().map(|r| json!(r)).collect::<Vec<_>>()
    );
}

#[test]
fn explicitly_missing_recovery_credentials_fail_closed() {
    let mut config = Config::default();
    config.agent.recovery.jev_route = Some("typesafe".into());
    assert!(resolved(&config, &|_| None).is_err());
}
