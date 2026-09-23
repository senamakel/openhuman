use super::*;

#[test]
fn recovery_tool_aliases_remain_stable() {
    assert!(is_recovery_tool(RETRIEVE_TOOL_NAME));
    assert!(is_recovery_tool(LEGACY_RETRIEVE_TOOL_NAME));
    assert!(!is_recovery_tool("shell"));
}

#[tokio::test]
async fn disabled_compaction_is_an_exact_pass_through_without_loading_the_module() {
    let content = "exact tool output".to_string();
    let output = compact_output_with_policy(
        content.clone(),
        "shell",
        false,
        AgentTokenjuiceCompression::Full,
    )
    .await;
    assert_eq!(output, content);
}

#[tokio::test]
async fn off_profile_is_an_exact_pass_through_without_loading_the_module() {
    let content = "exact tool output".to_string();
    let output = compact_output_with_policy(
        content.clone(),
        "shell",
        true,
        AgentTokenjuiceCompression::Off,
    )
    .await;
    assert_eq!(output, content);
}

/// The whole summary path over the real bus: `CompactWith` into the module,
/// `MlHost.Generate` back out to a registered call, the summary back in.
/// Runs where CI builds the module (`TINYJUICE_TEST_MODULE`); skipped
/// otherwise, since the pinned release may predate `CompactWith`.
#[tokio::test]
async fn the_module_calls_back_for_a_summary_written_for_the_focus() {
    // Serialized with every other test in this file that reads or mutates
    // `TINYJUICE_TEST_MODULE`, so a concurrently running env-mutating test
    // (e.g. `a_module_disabled_in_configuration_discloses_a_wanted_summary`)
    // cannot flip this check mid-read.
    let _lock = crate::config::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if std::env::var_os("TINYJUICE_TEST_MODULE").is_none() {
        return;
    }
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None::<types::GenerateRequest>));
    let sink = seen.clone();
    let ticket = generate::register(Box::new(move |request: types::GenerateRequest| {
        *sink.lock().unwrap() = Some(request);
        Box::pin(async { Ok("the rate limit is 60 requests a minute".to_string()) })
    }));
    // Over the default 4000-token threshold the test config installs.
    let content = "Rate limiting. Requests are limited per key. ".repeat(500);

    let output = compact_tool_output(ToolOutputCompaction {
        content: content.clone(),
        tool_name: "web_fetch",
        enabled: false,
        profile: AgentTokenjuiceCompression::Full,
        runtime_config: None,
        arguments: None,
        focus: Some("the rate limits".into()),
        context_token: Some(ticket.token().to_string()),
        scope: Some("module-summary-test".into()),
    })
    .await;

    assert_eq!(output.summarized_from_bytes, Some(content.len()));
    assert!(output
        .text
        .starts_with("the rate limit is 60 requests a minute"));
    assert!(
        output.text.contains(RETRIEVE_TOOL_NAME),
        "the original stays retrievable: {}",
        output.text
    );
    let request = seen
        .lock()
        .unwrap()
        .clone()
        .expect("the module called back");
    assert!(request.prompt.contains("Caller focus: the rate limits"));
    assert!(request.system.contains("caller focus"));
}

/// A result that was registered for a summary and never reached the module
/// still says it is unsummarized, exactly as a module-side failure would.
#[tokio::test]
async fn an_unreachable_module_discloses_a_wanted_summary() {
    if std::env::var_os("TINYJUICE_TEST_MODULE").is_some() {
        return; // A fixture forces the module on; this pins the host path.
    }
    let mut config = crate::config::Config::default();
    config.modules.enabled = false;
    let config = std::sync::Arc::new(config);
    let call = |context_token: Option<String>| ToolOutputCompaction {
        content: "raw".to_string(),
        tool_name: "web_fetch",
        enabled: false,
        profile: AgentTokenjuiceCompression::Light,
        runtime_config: Some(&config),
        arguments: None,
        focus: Some("the pricing".to_string()),
        context_token,
        scope: None,
    };

    let wanted = compact_tool_output(call(Some("token".to_string()))).await;
    assert_eq!(wanted.text, "raw");
    assert_eq!(wanted.notice, Some(summary_failed_notice()));

    let unwanted = compact_tool_output(ToolOutputCompaction {
        enabled: true,
        ..call(None)
    })
    .await;
    assert_eq!(unwanted.notice, None);
}

/// An agent whose profile bypasses TinyJuice's router entirely still
/// discloses a summary that was registered anyway (openhuman#6581 review):
/// the early `Off` return must not silently drop the notice the way an
/// ordinary `unchanged` return would.
#[tokio::test]
async fn an_off_profile_still_discloses_a_registered_summary() {
    let output = compact_tool_output(ToolOutputCompaction {
        content: "raw".to_string(),
        tool_name: "web_fetch",
        enabled: true,
        profile: AgentTokenjuiceCompression::Off,
        runtime_config: None,
        arguments: None,
        focus: None,
        context_token: Some("token".to_string()),
        scope: None,
    })
    .await;
    assert_eq!(output.text, "raw");
    assert_eq!(output.notice, Some(summary_failed_notice()));

    let silent = compact_tool_output(ToolOutputCompaction {
        content: "raw".to_string(),
        tool_name: "web_fetch",
        enabled: true,
        profile: AgentTokenjuiceCompression::Off,
        runtime_config: None,
        arguments: None,
        focus: None,
        context_token: None,
        scope: None,
    })
    .await;
    assert_eq!(silent.text, "raw");
    assert_eq!(silent.notice, None);
}

/// A module disabled in configuration cannot install, even when the process
/// also carries a `TINYJUICE_TEST_MODULE` fixture: the config check runs
/// first. Exercises the config-driven passthrough independent of whatever
/// module happens to be installed for other tests in this binary.
#[tokio::test]
async fn a_module_disabled_in_configuration_discloses_a_wanted_summary() {
    let _lock = crate::config::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var_os("TINYJUICE_TEST_MODULE");
    // SAFETY: serialized by TEST_ENV_LOCK; restored below for every exit path.
    unsafe { std::env::remove_var("TINYJUICE_TEST_MODULE") };
    struct RestoreEnv(Option<std::ffi::OsString>);
    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            // SAFETY: still under TEST_ENV_LOCK for the guarded test's scope.
            match self.0.take() {
                Some(value) => unsafe { std::env::set_var("TINYJUICE_TEST_MODULE", value) },
                None => unsafe { std::env::remove_var("TINYJUICE_TEST_MODULE") },
            }
        }
    }
    let _restore = RestoreEnv(previous);

    let mut config = crate::config::Config::default();
    config.modules.enabled = false;
    let config = std::sync::Arc::new(config);

    let output = compact_tool_output(ToolOutputCompaction {
        content: "raw".to_string(),
        tool_name: "web_fetch",
        enabled: true,
        profile: AgentTokenjuiceCompression::Light,
        runtime_config: Some(&config),
        arguments: None,
        focus: None,
        context_token: Some("token".to_string()),
        scope: None,
    })
    .await;

    assert_eq!(output.text, "raw");
    assert_eq!(
        output.notice,
        Some(summary_failed_notice()),
        "installation failure must disclose the unsummarized notice"
    );
}

#[test]
fn passthrough_discloses_a_notice_only_when_a_summary_was_wanted() {
    let silent = CompactedToolOutput::passthrough("raw".to_string(), false);
    assert_eq!(silent.text, "raw");
    assert_eq!(silent.notice, None);
    assert_eq!(silent.summarized_from_bytes, None);

    let disclosed = CompactedToolOutput::passthrough("raw".to_string(), true);
    assert_eq!(disclosed.text, "raw");
    assert_eq!(disclosed.notice, Some(summary_failed_notice()));
    assert_eq!(disclosed.summarized_from_bytes, None);
}

fn synthetic_compact_response(text: &str) -> types::CompactResponse {
    types::CompactResponse {
        text: text.to_string(),
        original_bytes: text.len(),
        compacted_bytes: text.len(),
        rule_id: "none/plain_text".into(),
        applied: false,
        content_kind: "plain_text".into(),
        compressor: "none".into(),
        original_tokens: text.len().div_ceil(4) as u64,
        compacted_tokens: text.len().div_ceil(4) as u64,
        notice: None,
    }
}

fn unknown_method_error() -> tinybus::Error {
    tinybus::Error::UnknownMethod {
        interface: tinybus::InterfaceName::new("ai.tinyhumans.tinyjuice.Compaction").unwrap(),
        member: tinybus::MemberName::new("CompactWith").unwrap(),
    }
}

fn method_failed_error() -> tinybus::Error {
    tinybus::Error::MethodFailed {
        name: "ai.tinyhumans.tinyjuice.Error.Internal".into(),
        message: "boom".into(),
    }
}

#[test]
fn a_successful_compact_with_reply_is_used_as_is() {
    let response = synthetic_compact_response("compacted");
    match classify_compact_with_reply(Ok(response.clone()), "shell") {
        CompactWithOutcome::Response(got) => assert_eq!(got.text, response.text),
        _ => panic!("expected Response"),
    }
}

#[test]
fn an_unknown_method_error_asks_to_retry_as_compact() {
    assert!(matches!(
        classify_compact_with_reply(Err(unknown_method_error()), "shell"),
        CompactWithOutcome::RetryAsCompact
    ));
}

#[test]
fn any_other_compact_with_error_gives_up() {
    assert!(matches!(
        classify_compact_with_reply(Err(method_failed_error()), "shell"),
        CompactWithOutcome::GiveUp
    ));
}

#[test]
fn a_legacy_compact_reply_gains_the_summary_notice_when_one_was_wanted_and_missing() {
    let response = synthetic_compact_response("legacy");
    let finished = finish_legacy_compact_reply(Ok(response), true)
        .expect("a successful reply is never dropped");
    assert_eq!(finished.notice, Some(summary_failed_notice()));
}

#[test]
fn a_legacy_compact_reply_keeps_its_own_notice_when_it_already_has_one() {
    let mut response = synthetic_compact_response("legacy");
    response.notice = Some("module notice".to_string());
    let finished =
        finish_legacy_compact_reply(Ok(response), true).expect("a successful reply is kept");
    assert_eq!(finished.notice, Some("module notice".to_string()));
}

#[test]
fn a_legacy_compact_reply_is_silent_when_no_summary_was_wanted() {
    let response = synthetic_compact_response("legacy");
    let finished = finish_legacy_compact_reply(Ok(response), false)
        .expect("a successful reply is never dropped");
    assert_eq!(finished.notice, None);
}

#[test]
fn a_failed_legacy_compact_reply_gives_up() {
    assert!(finish_legacy_compact_reply(Err(method_failed_error()), true).is_none());
}
