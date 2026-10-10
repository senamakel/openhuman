use super::*;

#[test]
fn shell_detects_python_runtime_commands() {
    for command in [
        "python3 -m pyfiglet hello",
        "python -m pip install pyfiglet",
        "pip install pyfiglet",
        "pip3.13 show pyfiglet",
        "/opt/openhuman/python/bin/python3 script.py",
        "echo hi && python3 -V",
    ] {
        assert!(
            shell_command_needs_python_runtime(command),
            "expected python runtime detection for {command}"
        );
    }

    for command in [
        "echo python3",
        "ls",
        "cat ./pipelines.txt",
        "node script.js",
    ] {
        assert!(
            !shell_command_needs_python_runtime(command),
            "did not expect python runtime detection for {command}"
        );
    }
}

#[test]
fn shell_runtime_path_prepends_managed_dirs_before_host_path() {
    let python = std::path::Path::new("/opt/openhuman/python/bin");
    let node = std::path::Path::new("/opt/openhuman/node/bin");
    let joined = prepend_path_dirs([python, node], "/usr/local/bin:/usr/bin");
    let sep = if cfg!(windows) { ";" } else { ":" };
    assert_eq!(
        joined,
        format!(
            "{}{}{}{}{}",
            python.display(),
            sep,
            node.display(),
            sep,
            "/usr/local/bin:/usr/bin"
        )
    );
}

/// Empirical answer to "does `shell` resolve/install managed Node on its
/// own?" — NO. The shell path consults the managed Node bootstrap only via
/// `try_cached()`, which never calls `resolve()` and therefore never
/// downloads/installs anything. So without a prior `node_exec` / `npm_exec`
/// (the tools that DO call `resolve()` and share this bootstrap instance),
/// `runtime_path_for_command` injects nothing for a node command. On a host
/// with no Node in the login PATH, the command then fails — the managed
/// runtime is never reached on the shell path. (Python, by contrast, IS
/// self-resolved in `runtime_path_for_command` — see the python branch.)
#[tokio::test]
async fn shell_does_not_resolve_or_install_node_on_its_own() {
    let node = Arc::new(NodeBootstrap::new(Arc::new(
        crate::config::Config::default(),
    )));
    let tool = ShellTool::with_language_bootstraps(
        test_security(AutonomyLevel::Full),
        test_runtime(),
        test_audit(),
        Some(node),
        None,
    );

    // Unprimed (no prior node_exec/npm_exec resolve): shell injects NO
    // managed node bin onto PATH — it does not auto-resolve or install.
    let injected = tool.runtime_path_for_command("node --version").await;
    assert!(
        injected.is_none(),
        "shell injected a managed node bin without any prior node_exec/npm_exec \
         resolve — it must not auto-resolve/install on the shell path: {injected:?}"
    );
}

/// A Python runtime that cannot be resolved must not fail the command: the
/// shell keeps the inherited `PATH`, so the host's own `python3` (a task
/// repo's interpreter with its dependencies) still runs it. It used to answer
/// every `python …` command with `Failed to resolve command runtime` once the
/// runtime module had faulted.
#[tokio::test]
async fn shell_keeps_inherited_path_when_python_runtime_is_unavailable() {
    let mut config = crate::config::Config::default();
    config.runtime_python.enabled = false;
    let python = Arc::new(PythonBootstrap::new(Arc::new(config)));
    assert!(
        python.resolve().await.is_err(),
        "a disabled python runtime must fail to resolve for this test to mean anything"
    );
    let tool = ShellTool::with_language_bootstraps(
        test_security(AutonomyLevel::Full),
        test_runtime(),
        test_audit(),
        None,
        Some(python),
    );
    assert_eq!(tool.runtime_path_for_command("python3 -V").await, None);

    let result = tool
        .execute(json!({"command": "python3 -c 'print(1)' || echo no-host-python"}))
        .await
        .unwrap();
    let output = result.output();
    assert!(
        !output.contains("Failed to resolve command runtime")
            && (output.contains('1') || output.contains("no-host-python")),
        "the command must run on the inherited PATH: {output}"
    );
}

#[test]
fn shell_runtime_failure_logging_accepts_enabled_and_disabled_runtime_states() {
    log_python_runtime_unavailable(true, &anyhow::anyhow!("test resolution failure"));
    log_python_runtime_unavailable(false, &anyhow::anyhow!("test disabled runtime"));
}

#[cfg(unix)]
#[derive(Default)]
struct TrackingNativeRuntime {
    native: NativeRuntime,
    builds: std::sync::atomic::AtomicUsize,
}

#[cfg(unix)]
impl RuntimeAdapter for TrackingNativeRuntime {
    fn name(&self) -> &str {
        self.native.name()
    }
    fn has_shell_access(&self) -> bool {
        self.native.has_shell_access()
    }
    fn has_filesystem_access(&self) -> bool {
        self.native.has_filesystem_access()
    }
    fn storage_path(&self) -> std::path::PathBuf {
        self.native.storage_path()
    }
    fn supports_long_running(&self) -> bool {
        self.native.supports_long_running()
    }
    fn build_shell_command(
        &self,
        command: &str,
        workspace: &std::path::Path,
    ) -> anyhow::Result<tokio::process::Command> {
        self.builds
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.native.build_shell_command(command, workspace)
    }
}

#[cfg(unix)]
fn shell_with_cached_python_using(
    runtime: Arc<dyn RuntimeAdapter>,
) -> (ShellTool, tempfile::TempDir) {
    use crate::runtime::python::{PythonSource, ResolvedPython};
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = tempfile::Builder::new()
        .prefix("managed-python's $(false) ")
        .tempdir()
        .unwrap();
    let python_bin = bin_dir.path().join("python3");
    std::fs::write(&python_bin, "#!/bin/sh\necho managed-python-path\n").unwrap();
    std::fs::set_permissions(&python_bin, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut config = crate::config::Config::default();
    config.runtime_python.enabled = true;
    let python = Arc::new(PythonBootstrap::new(Arc::new(config)));
    python.cache_for_test(ResolvedPython {
        bin_dir: bin_dir.path().to_path_buf(),
        python_bin,
        version: "test".into(),
        source: PythonSource::Managed,
    });
    (
        ShellTool::with_language_bootstraps(
            test_security(AutonomyLevel::Full),
            runtime,
            test_audit(),
            None,
            Some(python),
        ),
        bin_dir,
    )
}

#[cfg(unix)]
#[tokio::test]
async fn shell_uses_cached_python_path_in_native_mode() {
    use crate::agent::harness::definition::SandboxMode;
    use crate::agent::harness::with_current_sandbox_mode;

    let _env = EnvVarGuard::locked_set_async(crate::sandbox::ops::SANDBOX_OFF_ENV, "off").await;
    let runtime = Arc::new(TrackingNativeRuntime::default());
    let (tool, _bin_dir) = shell_with_cached_python_using(runtime.clone());
    let result = with_current_sandbox_mode(SandboxMode::None, async {
        tool.execute(json!({"command": "python3 -c 'print(1)'"}))
            .await
            .unwrap()
    })
    .await;
    assert!(
        !result.is_error,
        "managed Python command failed: {}",
        result.output()
    );
    assert!(result.output().contains("managed-python-path"));
    assert_eq!(
        runtime.builds.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "native execution must use the instrumented runtime adapter"
    );
}

#[tokio::test]
async fn shell_blocks_rate_limited() {
    let security = Arc::new(SecurityPolicy {
        autonomy: AutonomyLevel::Supervised,
        max_actions_per_hour: 0,
        workspace_dir: std::env::temp_dir(),
        action_dir: std::env::temp_dir(),
        ..SecurityPolicy::default()
    });
    let tool = ShellTool::new(security, test_runtime(), test_audit());
    let result = tool.execute(json!({"command": "echo test"})).await.unwrap();
    assert!(result.is_error);
    assert!(result.output().contains("Rate limit"));
}

#[cfg(not(windows))]
#[tokio::test]
async fn shell_sandboxed_mode_routes_through_sandbox_backend() {
    use crate::agent::harness::definition::SandboxMode;
    use crate::agent::harness::with_current_sandbox_mode;

    let _env = EnvVarGuard::locked_set_async(crate::sandbox::ops::SANDBOX_OFF_ENV, "on").await;
    let runtime = Arc::new(TrackingNativeRuntime::default());
    let (tool, _bin_dir) = shell_with_cached_python_using(runtime.clone());
    let result = with_current_sandbox_mode(SandboxMode::Sandboxed, async {
        tool.execute(json!({"command": "python3 -c 'print(1)'"}))
            .await
            .unwrap()
    })
    .await;
    assert!(
        !result.is_error,
        "sandboxed managed Python command should succeed: {}",
        result.output()
    );
    assert!(
        result.output().contains("managed-python-path"),
        "expected managed Python PATH in result, got: {:?}",
        result.output()
    );
    assert_eq!(
        runtime.builds.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "sandboxed execution must bypass the tool's native runtime adapter"
    );
}

/// Shell-family callers must use the shared routing decision, which covers
/// both an agent's sandbox mode and an explicit operator backend. The test
/// above executes a command and proves it bypasses the native runtime adapter;
/// this guard covers Node/npm call sites without downloading their runtimes.
#[test]
fn shell_family_tools_route_to_sandbox_when_sandboxed_mode_active() {
    const SHELL_SRC: &str = include_str!("shell.rs");
    const NODE_EXEC_SRC: &str = include_str!("node_exec.rs");
    const NPM_EXEC_SRC: &str = include_str!("npm_exec.rs");

    for (name, src) in [
        ("shell.rs", SHELL_SRC),
        ("node_exec.rs", NODE_EXEC_SRC),
        ("npm_exec.rs", NPM_EXEC_SRC),
    ] {
        assert!(
            src.contains("command_requires_sandbox().await"),
            "{name} must use the shared command sandbox routing policy, including agent mode and operator backend"
        );
        // Use the call-site pattern `.run_sandboxed(` so the assertion
        // doesn't trivially pass on the helper definition itself
        // (`fn run_sandboxed(...)`). If `execute()` / `run_with_security()`
        // stop delegating, this fires even though the helper still exists.
        assert!(
            src.contains(".run_sandboxed("),
            "{name} must delegate to a `run_sandboxed` helper when the sandbox mode is \
             active (see #3235). Whitespace before `.run_sandboxed` is tolerated; the \
             helper call must appear in the source — *not* just the helper definition."
        );
    }
}

#[test]
fn managed_path_restoration_preserves_cmd_syntax_and_unmanaged_commands() {
    use crate::agent::platform_shell::ShellFlavor;
    let windows_command = "echo %USERPROFILE%";
    assert_eq!(
        command_with_runtime_path(windows_command, Some("C:\\managed\\bin"), ShellFlavor::Cmd),
        windows_command
    );
    let unmanaged_command = "echo \"$PATH\"";
    assert_eq!(
        command_with_runtime_path(unmanaged_command, None, ShellFlavor::Posix),
        unmanaged_command
    );
}
