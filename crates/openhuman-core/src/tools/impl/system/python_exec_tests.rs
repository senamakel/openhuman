use super::*;

#[tokio::test]
async fn python_exec_blocks_protected_literal_before_runtime_resolution() {
    let tool = PythonExecTool::new(
        std::sync::Arc::new(crate::security::SecurityPolicy {
            enabled: false,
            ..crate::security::SecurityPolicy::default()
        }),
        std::sync::Arc::new(crate::agent::host_runtime::NativeRuntime::new()),
        std::sync::Arc::new(PythonBootstrap::new(std::sync::Arc::new(
            crate::config::Config::default(),
        ))),
        crate::config::RuntimePoolConfig::default(),
        std::path::PathBuf::from("."),
    );
    let result = tool
        .execute(json!({"inline_code": "open('~/.aws/credentials').read()"}))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.text().contains("protected path"));
}

#[test]
fn python_timeout_policy_unbounded_by_default() {
    assert_eq!(python_timeout_policy(&json!({})), ToolTimeout::Unbounded);
    assert_eq!(
        python_timeout_policy(&json!({"timeout_secs": 0})),
        ToolTimeout::Unbounded
    );
}

#[test]
fn python_timeout_policy_enforces_and_caps_explicit() {
    assert_eq!(
        python_timeout_policy(&json!({"timeout_secs": 120})),
        ToolTimeout::Millis(120_000)
    );
    assert_eq!(
        python_timeout_policy(&json!({"timeout_secs": 99999})),
        ToolTimeout::Millis(PYTHON_TIMEOUT_MAX_SECS * 1000)
    );
}

#[test]
fn shell_quote_escapes_single_quotes() {
    assert_eq!(shell_quote("it's"), "'it'\\''s'");
    assert_eq!(shell_quote("print('hi')"), "'print('\\''hi'\\'')'");
}

#[test]
fn resolve_script_path_rejects_escapes() {
    let ws = std::path::Path::new("/ws");
    assert!(resolve_script_path(ws, "").is_err());
    assert!(resolve_script_path(ws, "../evil.py").is_err());
    assert_eq!(
        resolve_script_path(ws, "scripts/run.py").unwrap(),
        std::path::Path::new("/ws/scripts/run.py")
    );
}

/// `python_exec` clears the child environment, so its allow-list must carry the
/// Windows process-bootstrap set — a child without them fails to start.
#[test]
fn safe_env_vars_cover_windows_bootstrap() {
    crate::agent::platform_shell::assert_forwards_windows_bootstrap(
        SAFE_ENV_VARS,
        "python_exec::SAFE_ENV_VARS",
    );
}
