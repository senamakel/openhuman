use super::*;
use tinyagents_harness::context::{RunConfig, RunContext};

fn context() -> RunContext<crate::agent::tinyagents::host::OpenHumanRunContext> {
    RunContext::new(
        RunConfig::new("tool-output-identity-test"),
        crate::agent::tinyagents::host::OpenHumanRunContext::new(),
    )
}

#[tokio::test]
async fn same_tool_calls_persist_artifacts_under_distinct_call_ids() {
    let temp = tempfile::tempdir().expect("temporary artifact root");
    let middleware = ToolOutputMiddleware {
        budget_bytes: 8,
        payload_summarizer: None,
        artifact_store: Some(ToolResultArtifactStore::new(
            temp.path().to_path_buf(),
            "identity-session",
        )),
        tokenjuice_compaction_enabled: false,
        tokenjuice_compression: AgentTokenjuiceCompression::Off,
        runtime_config: None,
        tool_policies: HashMap::new(),
        artifact_reads: Default::default(),
        focus_by_call: Default::default(),
        summary_focus_tools: Default::default(),
        raw_fetches: Default::default(),
    };
    let mut ctx = context();

    let first = ToolInvocationIdentity::new("echo-1", "echo");
    let mut first_result = TaToolResult::success("first result is deliberately oversized");
    middleware
        .after_tool(&mut ctx, &(), &first, &mut first_result)
        .await
        .expect("first artifact is persisted");

    let second = ToolInvocationIdentity::new("echo-2", "echo");
    let mut second_result = TaToolResult::success("second result is deliberately oversized");
    middleware
        .after_tool(&mut ctx, &(), &second, &mut second_result)
        .await
        .expect("second artifact is persisted");

    let root = temp
        .path()
        .join("artifacts/tool-results/identity-session/echo");
    assert_eq!(
        std::fs::read_to_string(root.join("echo-1.txt")).expect("first artifact"),
        "first result is deliberately oversized"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("echo-2.txt")).expect("second artifact"),
        "second result is deliberately oversized"
    );
}

/// `raw: true` asks `web_fetch` for the body as sent, which switches off the
/// HTML→Markdown conversion — so the payload is unconverted markup, and handing
/// it to the summarizer buys an uncached model call to paraphrase minified JS.
/// One observed fetch cost 44,561 prompt tokens that way. These pin which calls
/// earn the exemption, not what the ladder then does with them.
#[test]
fn only_a_raw_web_fetch_is_exempt_from_the_payload_summarizer() {
    use serde_json::json;

    assert!(is_raw_fetch(
        "web_fetch",
        &json!({"url": "https://x", "raw": true})
    ));

    // A converted fetch is the normal path and stays summarizer-eligible: its
    // Markdown is prose the summarizer compresses well.
    for args in [
        json!({"url": "https://x"}),
        json!({"url": "https://x", "raw": false}),
        json!({"url": "https://x", "raw": null}),
        // `raw` is a bool on the wire; a string is not a request for raw bytes.
        json!({"url": "https://x", "raw": "true"}),
    ] {
        assert!(
            !is_raw_fetch("web_fetch", &args),
            "{args} is a converted fetch"
        );
    }

    // The exemption is about `web_fetch`'s conversion, so a `raw` argument on
    // any other tool means nothing here.
    for tool in ["file_read", "shell", "http_request"] {
        assert!(
            !is_raw_fetch(tool, &json!({"raw": true})),
            "{tool} has no HTML conversion to switch off"
        );
    }
}

/// `use_skill` forwards the wrapped tool's result verbatim, so a raw fetch
/// reached through it is still a raw fetch — the same wrapper-following
/// `artifact_read_target` does.
#[test]
fn a_raw_fetch_wrapped_in_use_skill_is_still_a_raw_fetch() {
    use serde_json::json;

    assert!(is_raw_fetch(
        "use_skill",
        &json!({"skill": "web", "tool": "web_fetch", "args": {"url": "https://x", "raw": true}})
    ));
    assert!(!is_raw_fetch(
        "use_skill",
        &json!({"skill": "web", "tool": "web_fetch", "args": {"url": "https://x"}})
    ));
    // A wrapper naming some other tool, and a malformed one, are not raw
    // fetches — neither may silently inherit the exemption.
    assert!(!is_raw_fetch(
        "use_skill",
        &json!({"skill": "files", "tool": "file_read", "args": {"raw": true}})
    ));
    assert!(!is_raw_fetch("use_skill", &json!({"raw": true})));
}
