//! Title: Connect an actual MCP protocol stub over loopback
//! Summary: Connect an actual MCP protocol stub over loopback.
//! Run: offline with loopback stubs; no live path.
//! Feature: mcp

mod support;
use openhuman_embed::{AgentSpec, Runtime, Workspace};

fn main() -> anyhow::Result<()> {
    support::run(run())
}

async fn run() -> anyhow::Result<()> {
    let backend = support::stub_backend().await;
    let provider = support::provider("hello from the stub").await;
    let runtime = Runtime::builder()
        .config(support::offline_config())
        .workspace(Workspace::Ephemeral)
        .backend_url(backend.uri())
        .provider(support::route(&provider, "fixture"))
        .build()
        .await?;
    // ANCHOR: mcp
    let mcp = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(McpStub)
        .mount(&mcp)
        .await;
    let discovery = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::path("/v1/chat/completions"))
        .respond_with(ForecastModel(std::sync::atomic::AtomicUsize::new(0)))
        .mount(&discovery)
        .await;
    let agent = runtime.agent(
        AgentSpec::new("mcp-client")
            .provider(support::route(&discovery, "fixture"))
            .mcp(
                openhuman_embed::McpServer::http("forecast", format!("{}/mcp", mcp.uri()))
                    .allow_tools(["forecast"]),
            )
            .access(openhuman_embed::Access::full()),
    )?;
    assert_eq!(
        agent
            .run("List the forecast server tools, then call its forecast tool")
            .await?
            .reply,
        "sunny verified"
    );
    let requests = mcp.received_requests().await.expect("MCP requests");
    let methods: Vec<String> = requests
        .iter()
        .filter_map(|r| {
            serde_json::from_slice::<serde_json::Value>(&r.body).ok()?["method"]
                .as_str()
                .map(str::to_string)
        })
        .collect();
    assert!(methods.iter().any(|m| m == "initialize"), "{methods:?}");
    assert!(methods.iter().any(|m| m == "tools/list"), "{methods:?}");
    assert!(methods.iter().any(|m| m == "tools/call"), "{methods:?}");
    let requests = support::chat_requests(&discovery).await;
    assert!(support::tool_results(&requests[1]).contains("forecast"));
    assert!(support::tool_results(&requests[2]).contains("sunny"));
    println!("MCP handshake, discovery and remote execution verified");
    // ANCHOR_END: mcp
    support::passed("mcp");
    Ok(())
}

struct McpStub;
impl wiremock::Respond for McpStub {
    fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let result = match body["method"].as_str().unwrap_or_default() {
            "initialize" => {
                serde_json::json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"forecast","version":"1"}})
            }
            "notifications/initialized" => return wiremock::ResponseTemplate::new(202),
            "tools/list" => {
                serde_json::json!({"tools":[{"name":"forecast","description":"Local forecast","inputSchema":{"type":"object"}}]})
            }
            "tools/call" => serde_json::json!({"content":[{"type":"text","text":"sunny"}]}),
            _ => serde_json::json!({}),
        };
        wiremock::ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"jsonrpc":"2.0","id":body["id"],"result":result}))
    }
}

/// The fixture follows the immediately available MCP bridge and verifies results.
struct ForecastModel(std::sync::atomic::AtomicUsize);
impl wiremock::Respond for ForecastModel {
    fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let step = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let reply = match step {
            0 => support::tool_call_completion("mcp_list_tools", r#"{"server":"forecast"}"#),
            1 => {
                assert!(
                    support::tool_results(request).contains("forecast"),
                    "remote tools were listed successfully"
                );
                support::tool_call_completion(
                    "mcp_call_tool",
                    r#"{"server":"forecast","tool":"forecast","arguments":{}}"#,
                )
            }
            _ => {
                assert!(
                    support::tool_results(request).contains("sunny"),
                    "remote execution returned its result"
                );
                support::chat_completion("sunny verified")
            }
        };
        wiremock::ResponseTemplate::new(200).set_body_json(reply)
    }
}
