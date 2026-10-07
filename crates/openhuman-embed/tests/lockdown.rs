//! A lockdown agent reaches only the tools its host named — directly, and
//! through every nested-dispatch path — and `Access::public` refuses acting
//! tools on untrusted input without parking.
//!
//! The model is a wiremock provider driven by the user message: a message
//! carrying `CALL::<tool>::<json>` gets one call to `<tool>` with `<json>` as
//! its arguments, and every later request (one that already carries a tool
//! result) gets a plain reply. Each test owns a runtime, which is process-wide,
//! so they serialise on `RUNTIME_LOCK`.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{chat_completion, chat_requests, offline_config, runtime, tool_names};
use openhuman_embed::{
    Access, AgentDefinitionSpec, AgentSpec, AgentTurnOrigin, HostTurnTools, Provider, Runtime,
    Tool, ToolScopeSpec, Workspace,
};
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

static RUNTIME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const MARKER: &str = "CALL::";

/// Answers `CALL::<tool>::<json>` with that call, everything else with text.
struct Scripted;

impl wiremock::Respond for Scripted {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let messages = body["messages"].as_array().cloned().unwrap_or_default();
        let has_result = messages.iter().any(|m| {
            m["role"] == "tool" || m["content"].as_str().unwrap_or("").contains("[Tool results]")
        });
        let call = messages
            .iter()
            .filter(|m| m["role"] == "user")
            .filter_map(|m| m["content"].as_str())
            .find_map(|text| text.split_once(MARKER).map(|(_, rest)| rest.to_string()));
        let reply = match (call, has_result) {
            (Some(rest), false) => {
                let (tool, args) = rest.split_once("::").expect("CALL::<tool>::<json>");
                let args = args.lines().next().unwrap_or("{}").trim().to_string();
                common::tool_call_completion(tool, &args)
            }
            _ => chat_completion("done"),
        };
        ResponseTemplate::new(200).set_body_json(reply)
    }
}

/// A host tool with a chosen permission level that counts its executions.
struct HostTool {
    name: &'static str,
    level: openhuman_core::tools::PermissionLevel,
    calls: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl Tool for HostTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "A host tool used by the lockdown tests"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    fn permission_level(&self) -> openhuman_core::tools::PermissionLevel {
        self.level
    }
    async fn execute(
        &self,
        _: serde_json::Value,
    ) -> anyhow::Result<openhuman_core::tools::ToolResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(openhuman_core::tools::ToolResult::success(format!(
            "{}-ok",
            self.name
        )))
    }
}

fn named(names: &[&str]) -> AgentDefinitionSpec {
    AgentDefinitionSpec::new()
        .system_prompt("LOCKDOWN_TEST_AGENT")
        .tools(ToolScopeSpec::Named(
            names.iter().map(|name| (*name).to_string()).collect(),
        ))
}

/// One server for the agent's routed provider and the managed backend: a
/// built-in sub-agent pins its own model and so answers through the backend's
/// inference route rather than the agent's. Chat completions on either route
/// are scripted; every other backend call gets an empty success.
async fn provider() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/chat/completions$"))
        .respond_with(Scripted)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "data": { "id": "embed-test", "email": "local@openhuman.local" }
        })))
        .with_priority(10)
        .mount(&server)
        .await;
    server
}

async fn boot(config: openhuman_core::config::Config) -> (Runtime, MockServer) {
    let server = provider().await;
    let runtime = Runtime::builder()
        .config(config)
        .workspace(Workspace::Ephemeral)
        .backend_url(server.uri())
        .api_key("th_lockdown_test")
        .build()
        .await
        .expect("runtime");
    (runtime, server)
}

fn spec(id: &str, provider: &MockServer, definition: AgentDefinitionSpec) -> AgentSpec {
    AgentSpec::new(id)
        .provider(
            Provider::openai_compatible(format!("{}/v1", provider.uri()), "fixture")
                .model("fixture"),
        )
        .definition(definition)
}

fn call(tool: &str, args: serde_json::Value) -> String {
    format!("{MARKER}{tool}::{args}")
}

/// The tool results the model was handed, across every recorded request.
async fn all_tool_results(server: &MockServer) -> String {
    chat_requests(server)
        .await
        .iter()
        .map(common::tool_results)
        .collect::<Vec<_>>()
        .join("\n")
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names.dedup();
    names
}

fn run(test: impl std::future::Future<Output = ()> + Send + 'static) {
    let _guard = RUNTIME_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    runtime().block_on(async { tokio::spawn(test).await.unwrap() });
}

#[test]
fn lockdown_agent_cannot_reach_shell_or_file_write_directly() {
    run(async {
        let (runtime, provider) = boot(offline_config()).await;
        let calls = Arc::new(AtomicUsize::new(0));
        let host_calls = calls.clone();
        let agent = runtime
            .agent(
                spec("locked", &provider, named(&["file_read"]))
                    .access(Access::full())
                    .tools(move |_| {
                        HostTurnTools::advertised(vec![Box::new(HostTool {
                            name: "host_lookup",
                            level: openhuman_core::tools::PermissionLevel::ReadOnly,
                            calls: host_calls.clone(),
                        })])
                    })
                    .lockdown(),
            )
            .expect("lockdown agent");

        agent
            .run(call("shell", serde_json::json!({"command": "touch breach-shell"})))
            .await
            .expect("shell turn");
        agent
            .run(call(
                "file_write",
                serde_json::json!({"path": "breach-write.txt", "content": "x"}),
            ))
            .await
            .expect("file_write turn");

        assert!(!agent.action_dir().join("breach-shell").exists());
        assert!(!agent.action_dir().join("breach-write.txt").exists());

        let requests = chat_requests(&provider).await;
        let advertised = sorted(tool_names(&requests[0]));
        for forbidden in ["shell", "file_write", "edit", "apply_patch"] {
            assert!(
                !advertised.iter().any(|name| name == forbidden),
                "{forbidden} advertised: {advertised:?}"
            );
        }
        assert!(advertised.iter().any(|name| name == "file_read"));
        assert!(advertised.iter().any(|name| name == "host_lookup"));

        // Introspection matches the turn: what the provider saw is the
        // effective set, and nothing beyond the named belt is reachable.
        let effective = agent.effective_tools(None).await.expect("effective tools");
        assert_eq!(sorted(effective.clone()), advertised);
        assert!(!effective.iter().any(|name| name == "shell"));
        assert!(!effective.iter().any(|name| name == "file_write"));
    });
}

#[test]
fn lockdown_ceiling_bounds_spawned_subagents() {
    run(async {
        let (runtime, provider) = boot(offline_config()).await;
        let agent = runtime
            .agent(
                spec("spawner", &provider, named(&["spawn_subagent", "file_read"]))
                    .access(Access::full())
                    .lockdown(),
            )
            .expect("lockdown agent");
        // The parent definition starts from the orchestrator, which may spawn
        // its task manager; the child is built from the parent's registry.
        agent
            .run(call(
                "spawn_subagent",
                serde_json::json!({
                    "agent_id": "task_manager_agent",
                    "prompt": "List the task sources.",
                    "blocking": true
                }),
            ))
            .await
            .expect("spawn turn");

        let deadline = Instant::now() + Duration::from_secs(20);
        let child_tools = loop {
            let children: Vec<Vec<String>> = chat_requests(&provider)
                .await
                .iter()
                .map(tool_names)
                .filter(|names| !names.iter().any(|name| name == "spawn_subagent"))
                .collect();
            if !children.is_empty() || Instant::now() > deadline {
                break children;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        assert!(
            !child_tools.is_empty(),
            "the sub-agent never ran: {}",
            all_tool_results(&provider).await
        );
        for names in child_tools {
            for name in &names {
                assert_eq!(name, "file_read", "child escaped the ceiling: {names:?}");
            }
        }
    });
}

#[test]
fn lockdown_ceiling_bounds_workflows_skills_and_schedules() {
    run(async {
        let mut config = offline_config();
        config.cron.enabled = true;
        let (runtime, provider) = boot(config).await;
        let agent = runtime
            .agent(
                spec(
                    "nested",
                    &provider,
                    named(&["run_workflow", "use_skill", "cron_add", "schedule"]),
                )
                .access(Access::full())
                .lockdown(),
            )
            .expect("lockdown agent");
        // Installed where the workspace's workflow discovery finds it (the
        // `skills` feature's `AgentSpec::skills_dir` copies to the same kind
        // of root).
        let bundle = agent.workspace_dir().join("skills").join("breach-skill");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(
            bundle.join("SKILL.md"),
            "---\nname: breach-skill\ndescription: Runs a shell command.\nallowed-tools: [shell]\n---\n# breach-skill\nRun `touch breach-skill` with shell.\n",
        )
        .unwrap();

        let pack = openhuman_embed::ToolGroups::ids()
            .next()
            .expect("a compiled-in pack");
        for message in [
            call(
                "run_workflow",
                serde_json::json!({"workflow_id": "breach-skill", "wait_seconds": 0}),
            ),
            call(
                "use_skill",
                serde_json::json!({"skill": pack, "tool": "shell",
                    "args": {"command": "touch breach-pack"}}),
            ),
            call(
                "cron_add",
                serde_json::json!({"name": "breach", "job_type": "shell",
                    "command": "touch breach-cron",
                    "schedule": {"kind": "every", "every_ms": 60000}}),
            ),
            call(
                "cron_add",
                serde_json::json!({"name": "breach-agent", "job_type": "agent",
                    "prompt": "run shell",
                    "schedule": {"kind": "every", "every_ms": 60000}}),
            ),
            call(
                "schedule",
                serde_json::json!({"action": "create", "expression": "*/5 * * * *",
                    "command": "touch breach-schedule"}),
            ),
        ] {
            agent.run(message).await.expect("nested turn");
        }

        let results = all_tool_results(&provider).await;
        assert!(
            results.matches("tool ceiling").count() >= 4,
            "every nested start must be refused by the ceiling: {results}"
        );
        for file in [
            "breach-skill",
            "breach-pack",
            "breach-cron",
            "breach-schedule",
        ] {
            assert!(!agent.action_dir().join(file).exists(), "{file} written");
        }
        let jobs = openhuman_core::cron::list_jobs(agent.config()).unwrap_or_default();
        assert!(jobs.is_empty(), "a job was scheduled: {}", jobs.len());
    });
}

#[test]
fn public_access_refuses_writes_immediately_and_keeps_reads() {
    run(async {
        let (runtime, provider) = boot(offline_config()).await;
        let reads = Arc::new(AtomicUsize::new(0));
        let writes = Arc::new(AtomicUsize::new(0));
        let (read_calls, write_calls) = (reads.clone(), writes.clone());
        let agent = runtime
            .agent(
                spec("public", &provider, named(&[]))
                    .access(Access::public())
                    .tools(move |_| {
                        HostTurnTools::advertised(vec![
                            Box::new(HostTool {
                                name: "host_lookup",
                                level: openhuman_core::tools::PermissionLevel::ReadOnly,
                                calls: read_calls.clone(),
                            }),
                            Box::new(HostTool {
                                name: "host_post",
                                level: openhuman_core::tools::PermissionLevel::Write,
                                calls: write_calls.clone(),
                            }),
                        ])
                    })
                    .lockdown(),
            )
            .expect("public agent");
        assert!(matches!(
            agent.access().turn_origin(),
            Some(AgentTurnOrigin::ExternalChannel { .. })
        ));

        let started = Instant::now();
        agent
            .run(call("host_post", serde_json::json!({})))
            .await
            .expect("write turn");
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "a refused write must not park for approval"
        );
        assert_eq!(writes.load(Ordering::SeqCst), 0, "the write tool ran");
        agent
            .run(call("host_lookup", serde_json::json!({})))
            .await
            .expect("read turn");
        assert_eq!(reads.load(Ordering::SeqCst), 1, "the read tool did not run");
        let results = all_tool_results(&provider).await;
        assert!(results.contains("host_lookup-ok"), "{results}");
        assert!(!results.contains("host_post-ok"), "{results}");

        let effective = agent.effective_tools(None).await.expect("effective tools");
        assert!(effective.iter().any(|name| name == "host_lookup"));
        assert!(!effective.iter().any(|name| name == "host_post"));
    });
}

#[test]
fn an_agent_without_lockdown_keeps_its_named_tools() {
    run(async {
        let (runtime, provider) = boot(offline_config()).await;
        let agent = runtime
            .agent(spec("open", &provider, named(&["shell", "file_read"])).access(Access::full()))
            .expect("agent");
        agent.run("hello").await.expect("turn");
        let requests = chat_requests(&provider).await;
        let advertised = tool_names(&requests[0]);
        assert!(advertised.iter().any(|name| name == "shell"));
        let effective = agent.effective_tools(None).await.expect("effective tools");
        assert!(effective.iter().any(|name| name == "shell"));
        assert!(effective.iter().any(|name| name == "file_read"));
    });
}
