//! End-to-end proof of the two-step library API: one `Runtime`, several
//! independently configured `Agent`s.
//!
//! One `#[test]` for the same reason as `harness_embed.rs`: a runtime claims a
//! process-wide slot. Everything is asserted against the one runtime this
//! process builds.
//!
//! No live LLM call is made. Each agent gets its own `wiremock` provider, which
//! is what makes the isolation assertions possible: a turn that leaked to the
//! other agent's provider would be recorded on the wrong mock.

mod common;

#[cfg(feature = "mcp")]
use common::tool_results;
use common::{chat_completion, offline_config, runtime, stub_backend, tool_call_completion};
use openhuman_embed::{
    Access, AgentDefinitionSpec, AgentError, AgentSpec, Provider, Runtime, SandboxModeSpec,
    Workspace,
};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, ResponseTemplate};

const API_KEY: &str = "th_test_key";

/// A skills fixture with one bundle.
fn skills_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("skills fixture");
    let bundle = dir.path().join("alpha-skill");
    std::fs::create_dir_all(&bundle).expect("bundle dir");
    std::fs::write(
        bundle.join("SKILL.md"),
        "---\nname: alpha-skill\ndescription: A fixture skill.\n---\n# alpha-skill\n",
    )
    .expect("SKILL.md");
    dir
}

#[test]
fn one_runtime_hosts_independently_configured_agents() {
    let _ = env_logger::builder().is_test(true).try_init();

    let rt = runtime();
    rt.block_on(async {
        tokio::spawn(async move {
            let backend = wiremock::MockServer::start().await;
            // Managed inference for an agent with no route of its own: the
            // runtime's API key must arrive as a bearer, and nothing else.
            Mock::given(method("POST"))
                .and(path("/openai/v1/chat/completions"))
                .and(header(
                    "authorization",
                    format!("Bearer {API_KEY}").as_str(),
                ))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(chat_completion("managed-ok")),
                )
                .mount(&backend)
                .await;
            // Everything else the core might call incidentally.
            let _catch_all = stub_backend().await;
            Mock::given(wiremock::matchers::any())
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "success": true,
                    "data": { "id": "embed-test", "email": "local@openhuman.local" }
                })))
                .mount(&backend)
                .await;

            // Each provider first asks for `mcp_list_servers` once, so the
            // tool's result — which names the servers the agent can see —
            // comes back in the second request of that first turn.
            let provider_a = wiremock::MockServer::start().await;
            let provider_b = wiremock::MockServer::start().await;
            for server in [&provider_a, &provider_b] {
                Mock::given(method("POST"))
                    .and(path("/v1/chat/completions"))
                    .respond_with(
                        ResponseTemplate::new(200)
                            .set_body_json(tool_call_completion("mcp_list_servers", "{}")),
                    )
                    .up_to_n_times(1)
                    .mount(server)
                    .await;
            }
            Mock::given(method("POST"))
                .and(path("/v1/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_json(chat_completion("alpha-ok")))
                .mount(&provider_a)
                .await;
            Mock::given(method("POST"))
                .and(path("/v1/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_json(chat_completion("beta-ok")))
                .mount(&provider_b)
                .await;
            #[allow(unused_variables)]
            let skills = skills_fixture();
            let beta_action = tempfile::tempdir().expect("beta action dir");

            let runtime = std::sync::Arc::new(
                Runtime::builder()
                    .config(offline_config())
                    .workspace(Workspace::Ephemeral)
                    .backend_url(backend.uri())
                    .api_key(API_KEY)
                    .build()
                    .await
                    .expect("runtime builds"),
            );
            assert!(runtime.has_api_key());

            // Regression: `has_api_key()` must read the credential store live,
            // not a snapshot taken when the runtime was built — a host that
            // clears or (re)stores the key on a running runtime via
            // `runtime.core().auth()` needs this to reflect that immediately.
            runtime
                .core()
                .auth()
                .clear_api_key()
                .await
                .expect("clear api key");
            assert!(
                !runtime.has_api_key(),
                "has_api_key() must observe a live clear, not a build-time snapshot"
            );
            runtime
                .core()
                .auth()
                .store_api_key(API_KEY)
                .await
                .expect("restore api key");
            assert!(
                runtime.has_api_key(),
                "has_api_key() must observe a live store, not a build-time snapshot"
            );

            let workspace_dir = runtime.workspace_dir().to_path_buf();
            let root_dir = runtime.root_dir().to_path_buf();
            assert!(workspace_dir.is_dir());
            assert!(
                std::env::var("OPENHUMAN_CORE_RPC_URL").is_err(),
                "a library runtime must not bind an RPC listener"
            );

            // The API key authenticates the runtime without a user.
            let state = runtime.core().auth().state().await.expect("auth state");
            assert!(state.is_authenticated, "api key must count as signed in");
            assert!(state.user_id.is_none(), "an api key carries no user");
            assert!(state.is_api_key(), "state names the credential: {state:?}");
            assert!(
                runtime
                    .core()
                    .auth()
                    .token()
                    .await
                    .expect("token")
                    .is_none(),
                "no session token exists"
            );

            // ── alpha: read-only, own provider, own skills, an MCP server ──
            #[allow(unused_mut)]
            let mut alpha_spec = AgentSpec::new("alpha")
                .provider(
                    Provider::openai_compatible(format!("{}/v1", provider_a.uri()), "sk-a")
                        .model("alpha-model"),
                )
                .access(Access::readonly())
                .definition(AgentDefinitionSpec::new().sandbox(SandboxModeSpec::ReadOnly));
            #[cfg(feature = "skills")]
            {
                alpha_spec = alpha_spec.skills_dir(skills.path());
            }
            #[cfg(feature = "mcp")]
            {
                alpha_spec = alpha_spec.mcp(openhuman_embed::McpServer::stdio(
                    "alpha-mcp",
                    "true",
                    Vec::<String>::new(),
                ));
            }
            let alpha = runtime.agent(alpha_spec).expect("alpha instantiates");

            // ── beta: full autonomy, own provider, caller-chosen action dir ──
            let beta = runtime
                .agent(
                    AgentSpec::new("beta")
                        .provider(
                            Provider::openai_compatible(format!("{}/v1", provider_b.uri()), "sk-b")
                                .model("beta-model"),
                        )
                        .access(Access::full())
                        .action_dir(beta_action.path()),
                )
                .expect("beta instantiates");

            // ── gamma: no route → managed inference on the runtime's key ──
            let gamma = runtime
                .agent(AgentSpec::new("gamma").access(Access::readonly()))
                .expect("gamma instantiates");

            assert_eq!(
                runtime.agent_ids(),
                vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
            );

            // Layout: every agent has its own home, transcripts and action dir.
            assert_eq!(alpha.home_dir(), workspace_dir.join("personalities/alpha"));
            assert_eq!(alpha.transcripts_dir(), workspace_dir.join("session_raw"));
            assert_eq!(alpha.action_dir(), root_dir.join("agents/alpha/action"));
            assert!(alpha.action_dir().is_dir());
            assert_eq!(beta.action_dir(), beta_action.path());
            assert!(
                !alpha.action_dir().starts_with(&workspace_dir),
                "action_dir must not sit inside the workspace"
            );
            #[cfg(feature = "skills")]
            {
                assert!(
                    alpha.skills_dir().join("alpha-skill/SKILL.md").is_file(),
                    "alpha's bundle lands in its profile-local skills root"
                );
                assert!(
                    !beta.skills_dir().join("alpha-skill").exists(),
                    "beta never sees alpha's skills"
                );
                assert!(
                    !workspace_dir.join("skills/alpha-skill").exists(),
                    "the shared workspace skills root stays untouched"
                );
            }
            #[cfg(feature = "mcp")]
            {
                assert_eq!(alpha.config().mcp_client.servers.len(), 1);
                assert!(beta.config().mcp_client.servers.is_empty());
            }
            assert_eq!(
                alpha.config().autonomy.level,
                openhuman_core::security::AutonomyLevel::ReadOnly
            );
            assert_eq!(
                beta.config().autonomy.level,
                openhuman_core::security::AutonomyLevel::Full
            );
            assert!(alpha.access().turn_origin().is_none());
            assert!(beta.access().turn_origin().is_some());

            // Turns land on each agent's own provider. The first turn on each
            // is the `mcp_list_servers` probe: two requests (tool call, then
            // the final text).
            let a0 = alpha.run("which mcp servers?").await.expect("alpha probe");
            assert!(a0.reply.contains("alpha-ok"), "{:?}", a0.reply);
            assert_eq!(provider_a.received_requests().await.unwrap().len(), 2);
            assert_eq!(provider_b.received_requests().await.unwrap().len(), 0);
            let b0 = beta.run("which mcp servers?").await.expect("beta probe");
            assert!(b0.reply.contains("beta-ok"), "{:?}", b0.reply);
            assert_eq!(provider_b.received_requests().await.unwrap().len(), 2);

            let a1 = alpha.run("hello from alpha").await.expect("alpha turn");
            assert!(a1.reply.contains("alpha-ok"), "{:?}", a1.reply);
            assert_eq!(provider_a.received_requests().await.unwrap().len(), 3);
            assert_eq!(provider_b.received_requests().await.unwrap().len(), 2);

            // The request bodies carry each agent's own model, and the MCP
            // servers each agent can list are its own (plus the host-seeded
            // documentation server every agent gets, as on the desktop).
            let a_reqs = provider_a.received_requests().await.unwrap();
            let b_reqs = provider_b.received_requests().await.unwrap();
            let a_body: serde_json::Value = serde_json::from_slice(&a_reqs[0].body).unwrap();
            let b_body: serde_json::Value = serde_json::from_slice(&b_reqs[0].body).unwrap();
            assert_eq!(a_body["model"], "alpha-model");
            assert_eq!(b_body["model"], "beta-model");
            #[cfg(feature = "mcp")]
            {
                let a_servers = tool_results(&a_reqs[1]);
                let b_servers = tool_results(&b_reqs[1]);
                assert!(
                    a_servers.contains("alpha-mcp"),
                    "alpha must see the server it declared: {a_servers}"
                );
                assert!(
                    !b_servers.contains("alpha-mcp"),
                    "beta must not see alpha's server: {b_servers}"
                );
                assert!(
                    a_servers.contains("gitbooks") && b_servers.contains("gitbooks"),
                    "both see the host-seeded docs server: {a_servers} / {b_servers}"
                );
            }

            // Transcripts are keyed by agent, and a thread resumes its own history.
            let transcripts: Vec<String> = std::fs::read_dir(alpha.transcripts_dir())
                .expect("transcript dir")
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert!(
                transcripts.iter().any(|f| f.ends_with("_alpha.jsonl")),
                "alpha's transcript carries its id: {transcripts:?}"
            );
            assert!(
                transcripts.iter().any(|f| f.ends_with("_beta.jsonl")),
                "beta's transcript carries its id: {transcripts:?}"
            );
            let a2 = alpha
                .turn("and again")
                .session(&a1.session_id)
                .send()
                .await
                .expect("alpha second turn");
            assert_eq!(a2.session_id, a1.session_id);
            let a2_req = provider_a.received_requests().await.unwrap().remove(3);
            let a2_body: serde_json::Value = serde_json::from_slice(&a2_req.body).unwrap();
            let a2_messages = a2_body["messages"].as_array().cloned().unwrap_or_default();
            assert!(
                a2_messages.iter().any(|m| m["content"]
                    .as_str()
                    .is_some_and(|c| c.contains("hello from alpha"))),
                "the second turn on a thread must carry the first exchange: {a2_messages:?}"
            );

            // Managed inference authenticates with the runtime's API key.
            let g1 = gamma.run("hello from gamma").await.expect("gamma turn");
            assert!(g1.reply.contains("managed-ok"), "{:?}", g1.reply);
            let managed: Vec<_> = backend
                .received_requests()
                .await
                .unwrap()
                .into_iter()
                .filter(|r| r.url.path() == "/openai/v1/chat/completions")
                .collect();
            assert_eq!(managed.len(), 1, "one managed completion");
            assert_eq!(
                managed[0]
                    .headers
                    .get("authorization")
                    .map(|v| v.to_str().unwrap()),
                Some(format!("Bearer {API_KEY}").as_str())
            );
            assert!(
                managed[0].headers.get("x-api-key").is_none(),
                "inference sends the key as a bearer only"
            );

            // Interleaved concurrency across agents keeps their providers apart.
            let mut turns = tokio::task::JoinSet::new();
            for index in 0..25 {
                let alpha = alpha.clone();
                let beta = beta.clone();
                turns.spawn(async move { alpha.turn(format!("a{index}")).send().await });
                turns.spawn(async move { beta.turn(format!("b{index}")).send().await });
            }
            let mut a_count = 0;
            let mut b_count = 0;
            while let Some(outcome) = turns.join_next().await {
                let outcome = outcome.expect("no panic").expect("turn runs");
                if outcome.reply.contains("alpha-ok") {
                    a_count += 1;
                } else if outcome.reply.contains("beta-ok") {
                    b_count += 1;
                } else {
                    panic!("unexpected reply {:?}", outcome.reply);
                }
            }
            assert_eq!((a_count, b_count), (25, 25));
            assert_eq!(provider_a.received_requests().await.unwrap().len(), 4 + 25);
            assert_eq!(provider_b.received_requests().await.unwrap().len(), 2 + 25);

            // Ids are unique while alive, and validated.
            let err = runtime
                .agent(AgentSpec::new("alpha"))
                .expect_err("duplicate id");
            assert!(
                matches!(err, AgentError::DuplicateId(ref id) if id == "alpha"),
                "{err:?}"
            );
            let err = runtime
                .agent(AgentSpec::new("Not Valid!"))
                .expect_err("invalid id");
            assert!(matches!(err, AgentError::InvalidId { .. }), "{err:?}");
            let err = runtime
                .agent(AgentSpec::new("wide").domains(openhuman_embed::DomainSet::full()))
                .expect_err("widening the runtime");
            assert!(matches!(err, AgentError::WidensRuntime(_)), "{err:?}");

            // Dropping every handle releases the id.
            drop(gamma);
            assert_eq!(
                runtime.agent_ids(),
                vec!["alpha".to_string(), "beta".to_string()]
            );
            let _gamma_again = runtime
                .agent(AgentSpec::new("gamma"))
                .expect("id reusable after drop");

            drop(alpha);
            drop(beta);
            drop(_gamma_again);
            let Ok(runtime) = std::sync::Arc::try_unwrap(runtime) else {
                panic!("the test is the runtime's sole owner once every agent is dropped");
            };
            drop(runtime);
            assert!(
                !workspace_dir.exists(),
                "an ephemeral workspace must be removed with its runtime"
            );
        })
        .await
        .expect("library host task did not panic");
    });
}
