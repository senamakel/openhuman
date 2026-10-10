//! Platform-aware shell description and the child environment builder
//! (`shell_platform.rs`), exercised through the `shell` tool.
//!
//! Both take the platform / parent environment as a parameter so the Windows
//! behaviour is pinned on every CI host, not only on a Windows runner.

use super::*;
use crate::agent::platform_shell::ShellFlavor;
use std::collections::HashMap;
use std::ffi::OsString;

#[test]
fn windows_description_names_cmd_and_its_equivalents_of_posix_commands() {
    // Production (0.64.x): models sent `pwd; ls` and `cat` to cmd.exe because
    // the description read like a POSIX shell.
    let desc = shell_description(ShellFlavor::Cmd);
    assert!(desc.contains("cmd.exe"), "{desc}");
    for equivalent in ["`cd`", "`dir`", "`type`", "`findstr`", "`%VAR%`", "`&&`"] {
        assert!(desc.contains(equivalent), "missing {equivalent}: {desc}");
    }
    assert!(
        desc.contains("powershell"),
        "names the PowerShell escape hatch: {desc}"
    );
    // No POSIX app-launch examples on Windows.
    assert!(!desc.contains("xdg-open"), "{desc}");
    assert!(!desc.contains("open -a"), "{desc}");
}

#[test]
fn posix_description_does_not_mention_cmd() {
    let desc = shell_description(ShellFlavor::Posix);
    assert!(!desc.contains("cmd.exe"), "{desc}");
    assert!(desc.contains("print what you need"), "{desc}");
}

#[test]
fn every_description_keeps_the_stdout_only_warning() {
    for flavor in [ShellFlavor::Cmd, ShellFlavor::Posix] {
        assert!(
            shell_description(flavor).contains("Only stdout/stderr comes back"),
            "{flavor:?}"
        );
    }
}

#[test]
fn the_tool_describes_the_shell_this_host_actually_spawns() {
    let tool = ShellTool::new(
        test_security(AutonomyLevel::Supervised),
        test_runtime(),
        test_audit(),
    );
    assert_eq!(
        tool.description(),
        shell_description(ShellFlavor::current())
    );
    let command_desc = tool.parameters_schema()["properties"]["command"]["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        command_desc,
        command_param_description(ShellFlavor::current())
    );
    assert!(command_param_description(ShellFlavor::Cmd).contains("cmd.exe"));
}

fn parent(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let map: HashMap<String, OsString> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), OsString::from(v)))
        .collect();
    move |name: &str| map.get(name).cloned()
}

fn as_map(env: Vec<(&'static str, OsString)>) -> HashMap<&'static str, String> {
    env.into_iter()
        .map(|(k, v)| (k, v.to_string_lossy().into_owned()))
        .collect()
}

#[test]
fn python_children_default_to_utf8_io() {
    // Production: `UnicodeEncodeError: 'charmap'` from Python under cmd.exe,
    // whose console code page is not UTF-8.
    let env = as_map(shell_child_env(parent(&[("PATH", "/bin")])));
    assert_eq!(env.get("PYTHONUTF8").map(String::as_str), Some("1"));
    assert_eq!(
        env.get("PYTHONIOENCODING").map(String::as_str),
        Some("utf-8")
    );
    assert_eq!(env.get("PATH").map(String::as_str), Some("/bin"));
}

#[test]
fn a_users_own_python_encoding_settings_win() {
    let env = as_map(shell_child_env(parent(&[
        ("PYTHONUTF8", "0"),
        ("PYTHONIOENCODING", "latin-1"),
    ])));
    assert_eq!(env.get("PYTHONUTF8").map(String::as_str), Some("0"));
    assert_eq!(
        env.get("PYTHONIOENCODING").map(String::as_str),
        Some("latin-1")
    );
}

#[test]
fn the_child_env_forwards_only_the_allow_list_plus_python_defaults() {
    let env = as_map(shell_child_env(parent(&[
        ("PATH", "/bin"),
        ("USERPROFILE", "C:\\Users\\me"),
        ("OPENAI_API_KEY", "sk-secret"),
        ("AWS_SECRET_ACCESS_KEY", "secret"),
    ])));
    assert_eq!(
        env.get("USERPROFILE").map(String::as_str),
        Some("C:\\Users\\me")
    );
    assert!(!env.contains_key("OPENAI_API_KEY"));
    assert!(!env.contains_key("AWS_SECRET_ACCESS_KEY"));
    for key in env.keys() {
        assert!(
            SAFE_ENV_VARS.contains(key) || PYTHON_UTF8_DEFAULTS.iter().any(|(k, _)| k == key),
            "unexpected {key}"
        );
    }
}

#[test]
fn sandboxed_calls_get_the_same_python_defaults() {
    let env = as_map(python_utf8_env(parent(&[])));
    assert_eq!(env.get("PYTHONUTF8").map(String::as_str), Some("1"));
    assert_eq!(
        env.get("PYTHONIOENCODING").map(String::as_str),
        Some("utf-8")
    );
}

#[tokio::test]
async fn a_spawned_shell_sees_the_python_utf8_defaults() {
    // A developer's own setting wins, so expect it when present.
    let expected = std::env::var("PYTHONUTF8").unwrap_or_else(|_| "1".to_string());
    let tool = ShellTool::new(test_security_with_env_cmd(), test_runtime(), test_audit());
    let command = if cfg!(windows) {
        "echo %PYTHONUTF8%"
    } else {
        "echo $PYTHONUTF8"
    };
    let result = tool
        .execute(serde_json::json!({ "command": command }))
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.output());
    assert_eq!(result.output().trim(), expected);
}
