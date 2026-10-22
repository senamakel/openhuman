//! A route's provider options and reasoning effort reach every model call it
//! carries: the agent's own turns, a per-turn route, a scheduled run and a
//! stateless completion, beneath whatever options the call itself names.

mod common;

use common::{chat_completion, chat_requests, offline_config, provider, runtime, stub_backend};
use openhuman_embed::{
    AgentSpec, ChatMessage, Completer, CompletionRequest, JobSchedule, JobSpec, Provider,
    ReasoningEffort, Route, Runtime, Workspace,
};
use serde_json::{json, Value};

fn bodies(requests: &[wiremock::Request]) -> Vec<Value> {
    requests
        .iter()
        .map(|request| serde_json::from_slice(&request.body).expect("json body"))
        .collect()
}

fn tuned(server: &wiremock::MockServer) -> Route {
    Route::openai_compatible(format!("{}/v1", server.uri()), "sk-tuned")
        .provider_options(json!({"provider": {"order": ["first", "second"]}, "top_k": 3}))
        .reasoning_effort(ReasoningEffort::Low)
}

#[test]
fn route_provider_options_and_reasoning_effort_reach_every_call_on_the_route() {
    runtime().block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let backend = stub_backend().await;
    let (tuned_provider, plain_provider, turn_provider) =
        tokio::join!(provider("tuned"), provider("plain"), provider("turn"));
    let runtime = Runtime::builder()
        .config(offline_config())
        .workspace(Workspace::Ephemeral)
        .backend_url(backend.uri())
        .build()
        .await
        .unwrap();
    let tuned_agent = runtime
        .agent(
            AgentSpec::new("tuned")
                .provider(Provider::routed(tuned(&tuned_provider)).model("fixture")),
        )
        .unwrap();
    let plain_agent = runtime
        .agent(AgentSpec::new("plain").provider(
            Provider::openai_compatible(format!("{}/v1", plain_provider.uri()), "sk-plain")
                .model("fixture"),
        ))
        .unwrap();

    // route_reasoning_effort_reaches_the_wire, and the route's options ride
    // along on the agent's own turn.
    tuned_agent.run("hello").await.unwrap();
    // request_provider_options_still_override_route_defaults: the turn's own
    // `provider` replaces the route's, while `top_k` keeps the route's value.
    tuned_agent
        .turn("again")
        .provider_options(json!({"provider": {"sort": "throughput"}}))
        .send()
        .await
        .unwrap();
    // cron_run_on_an_ephemeral_route_sends_its_provider_options.
    let cron = runtime.cron();
    cron.upsert(
        JobSpec::agent(
            "tuned-hourly",
            "tuned",
            "check in",
            JobSchedule::Every { ms: 3_600_000 },
        )
        .retries(0),
    )
    .unwrap();
    let run = cron.run_now("tuned-hourly").await.expect("job runs");
    assert!(run.success, "{}", run.output);

    let tuned_bodies = bodies(&chat_requests(&tuned_provider).await);
    assert_eq!(tuned_bodies.len(), 3, "{tuned_bodies:#?}");
    for (index, body) in tuned_bodies.iter().enumerate() {
        assert_eq!(body["top_k"], 3, "call {index}: {body}");
        assert_eq!(body["reasoning_effort"], "low", "call {index}: {body}");
    }
    assert_eq!(
        tuned_bodies[0]["provider"],
        json!({"order": ["first", "second"]})
    );
    assert_eq!(tuned_bodies[1]["provider"], json!({"sort": "throughput"}));
    assert_eq!(
        tuned_bodies[2]["provider"],
        json!({"order": ["first", "second"]})
    );

    // A route without options sends neither, unchanged from before.
    plain_agent.run("hello").await.unwrap();
    let plain_bodies = bodies(&chat_requests(&plain_provider).await);
    assert_eq!(plain_bodies.len(), 1);
    assert!(plain_bodies[0].get("provider").is_none(), "{}", plain_bodies[0]);
    assert!(plain_bodies[0].get("top_k").is_none(), "{}", plain_bodies[0]);
    assert!(
        plain_bodies[0].get("reasoning_effort").is_none(),
        "{}",
        plain_bodies[0]
    );

    // A per-turn route carries its own options for that turn only.
    plain_agent
        .turn("routed")
        .route(tuned(&turn_provider))
        .send()
        .await
        .unwrap();
    let turn_bodies = bodies(&chat_requests(&turn_provider).await);
    assert_eq!(turn_bodies.len(), 1);
    assert_eq!(
        turn_bodies[0]["provider"],
        json!({"order": ["first", "second"]})
    );
    assert_eq!(turn_bodies[0]["reasoning_effort"], "low");
    plain_agent.run("unrouted").await.unwrap();
    let plain_bodies = bodies(&chat_requests(&plain_provider).await);
    assert_eq!(plain_bodies.len(), 2);
    assert!(plain_bodies[1].get("provider").is_none(), "{}", plain_bodies[1]);

    drop(tuned_agent);
    drop(plain_agent);
    drop(runtime);
}

#[tokio::test]
async fn a_completion_sends_its_routes_options_beneath_its_own() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/chat/completions"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(chat_completion("ok")))
        .mount(&server)
        .await;
    let completer = Completer::new(tuned(&server));
    completer
        .complete(CompletionRequest::new(
            "fixture",
            vec![ChatMessage::user("hi")],
        ))
        .await
        .unwrap();
    completer
        .complete(
            CompletionRequest::new("fixture", vec![ChatMessage::user("hi")])
                .provider_options(json!({"provider": {"only": ["third"]}})),
        )
        .await
        .unwrap();
    let bodies = bodies(&chat_requests(&server).await);
    assert_eq!(bodies[0]["provider"], json!({"order": ["first", "second"]}));
    assert_eq!(bodies[0]["top_k"], 3);
    assert_eq!(bodies[0]["reasoning_effort"], "low");
    assert_eq!(bodies[1]["provider"], json!({"only": ["third"]}));
    assert_eq!(bodies[1]["top_k"], 3);
}
