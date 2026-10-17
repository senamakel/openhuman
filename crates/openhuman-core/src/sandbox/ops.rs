//! Sandbox backend operations — policy resolution, backend creation, and
//! routed execution.

use super::docker;
use super::types::{
    ElevatedOp, SandboxBackendHandle, SandboxBackendKind, SandboxExecRequest, SandboxExecResult,
    SandboxPolicy, SandboxStatus, ELEVATED_TOOLS,
};
use crate::config::{RuntimeConfig, SandboxBackend, SandboxConfig};
use crate::core::runtime::DispatchCtx;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;
use tinyagents_definition::SandboxMode;

/// Safe environment variables forwarded into sandboxed execution.
///
/// This is the Docker container's host-environment allowlist and the set the
/// TinyBox module's own safe snapshot starts from for every native shell child
/// ([`execute_unsandboxed`] and the native jail adapter): the module reads these
/// names from the parent and adds the Windows process-bootstrap variables
/// itself. Keeping the bootstrap names out of this policy prevents Windows paths
/// from being passed into Linux Docker containers.
pub const SANDBOX_ENV_PASSTHROUGH: &[&str] = &[
    "PATH", "HOME", "TERM", "LANG", "LC_ALL", "LC_CTYPE", "USER", "SHELL", "TMPDIR",
];

/// Host switch that turns the agent sandbox off for the whole process.
///
/// For hosts that already isolate the core (a container, a CI or benchmark
/// task image, a VM): the OS jail confines commands to the action dir, which
/// breaks work such as installing packages or editing `/etc` that the outer
/// isolation already permits. `off`, `none`, `0`, `false` or `disabled`
/// (case-insensitive) disable it; anything else, or unset, leaves it on.
pub const SANDBOX_OFF_ENV: &str = "OPENHUMAN_SANDBOX";

/// Whether `value` (the `OPENHUMAN_SANDBOX` setting) disables the sandbox.
pub fn sandbox_off_value(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("off" | "none" | "0" | "false" | "disabled")
    )
}

/// Whether a command tool must use the sandbox execution path. An explicit
/// backend in the active config is an operator requirement even when the
/// agent definition uses the usual `SandboxMode::None` default.
pub async fn command_requires_sandbox(
    ctx: &DispatchCtx,
    sandbox_mode: Option<SandboxMode>,
) -> Result<bool, String> {
    if crate::core::runtime::is_saas() {
        // Every SaaS command must enter the per-user container path, including
        // language tools whose agent definition does not request a sandbox.
        return Ok(true);
    }
    if sandbox_disabled_by_host(ctx) {
        return Ok(false);
    }
    if matches!(sandbox_mode, Some(SandboxMode::Sandboxed)) {
        return Ok(true);
    }
    let config = ctx.load_config().await?;
    if config_disables_sandbox(&config) {
        return Ok(false);
    }
    Ok(matches!(
        config.sandbox.backend,
        SandboxBackend::Landlock | SandboxBackend::Docker
    ) || config.runtime.kind == "docker")
}

/// Resolve the policy for a command tool before building its command line.
/// SaaS always uses the user's container; other hosts apply the operator's
/// backend selection while retaining an agent-level sandbox requirement.
pub(crate) async fn resolve_command_policy(
    ctx: &DispatchCtx,
    action_dir: &Path,
    state_dir: &Path,
) -> Result<SandboxPolicy, String> {
    if crate::core::runtime::is_saas() {
        return crate::profiles::tools::sandbox_policy(action_dir, state_dir);
    }
    let config = ctx
        .load_config()
        .await
        .map_err(|error| format!("Cannot read sandbox configuration: {error}"))?;
    resolve_command_policy_with_config(ctx, &config, action_dir, state_dir).await
}

/// [`resolve_command_policy`] for a caller that already holds the loaded
/// configuration (the scheduler). SaaS still wins over everything else.
pub(crate) async fn resolve_command_policy_with_config(
    ctx: &DispatchCtx,
    config: &crate::config::Config,
    action_dir: &Path,
    state_dir: &Path,
) -> Result<SandboxPolicy, String> {
    if crate::core::runtime::is_saas() {
        return crate::profiles::tools::sandbox_policy(action_dir, state_dir);
    }
    let mut policy = resolve_sandbox_policy(
        ctx,
        SandboxMode::Sandboxed,
        action_dir,
        state_dir,
        &config.runtime,
        false,
    );
    if !config_disables_sandbox(config) && !sandbox_disabled_by_host(ctx) {
        apply_requested_backend(&mut policy, &config.sandbox, &config.runtime, config)
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(policy)
}

/// Whether the host (env switch or `[components] sandbox = false`) turned the
/// sandbox off. Never true under SaaS: the mode check precedes every operator
/// switch, and `boot_guard` additionally refuses to boot a SaaS core that sets
/// `OPENHUMAN_SANDBOX=off`.
pub(crate) fn sandbox_disabled_by_host(ctx: &DispatchCtx) -> bool {
    if crate::core::runtime::is_saas() {
        return false;
    }
    sandbox_off_value(std::env::var(SANDBOX_OFF_ENV).ok().as_deref()) || sandbox_component_off(ctx)
}

/// `[components] sandbox = false` in the context `ctx` runs under. With no
/// context the component is on, since the set only narrows.
pub(crate) fn sandbox_component_off(ctx: &DispatchCtx) -> bool {
    let off = !ctx.components().sandbox;
    if off {
        tracing::debug!("[sandbox] component 'sandbox' disabled, using the unsandboxed fallback");
    }
    off
}

/// The saved config says no isolation: the legacy `enabled = false` flag or
/// `[components] sandbox = false`.
pub(crate) fn config_disables_sandbox(config: &crate::config::Config) -> bool {
    config.sandbox.enabled == Some(false)
        || !crate::core::runtime::ComponentSet::from_config(&config.components).sandbox
}

/// Resolve a `SandboxPolicy` from the agent's `SandboxMode`, the
/// session origin, and the global runtime config.
///
/// Non-main sessions (channel, cron, remote) default to `Docker` when
/// the mode is `Sandboxed` and Docker is configured. Local interactive
/// sessions default to `Local` (the TinyBox module's native process jail).
///
/// `state_dir` is the core's internal `workspace_dir`; host-side scratch such
/// as output capture goes there instead of into `action_dir`.
pub fn resolve_sandbox_policy(
    ctx: &DispatchCtx,
    mode: SandboxMode,
    action_dir: &Path,
    state_dir: &Path,
    runtime_config: &RuntimeConfig,
    is_remote_session: bool,
) -> SandboxPolicy {
    let backend = match mode {
        SandboxMode::None => SandboxBackendKind::None,
        SandboxMode::ReadOnly => SandboxBackendKind::None,
        SandboxMode::Sandboxed if sandbox_disabled_by_host(ctx) => {
            tracing::debug!(
                env = SANDBOX_OFF_ENV,
                "[sandbox] host disabled the sandbox; running sandboxed mode unconfined"
            );
            SandboxBackendKind::None
        }
        SandboxMode::Sandboxed => {
            if runtime_config.kind == "docker" || is_remote_session {
                SandboxBackendKind::Docker
            } else {
                SandboxBackendKind::Local
            }
        }
    };

    let docker_overrides = if backend == SandboxBackendKind::Docker {
        let dc = &runtime_config.docker;
        Some(super::types::DockerOverrides {
            image: Some(dc.image.clone()),
            network: Some(dc.network.clone()),
            memory_limit_mb: dc.memory_limit_mb,
            cpu_limit: dc.cpu_limit,
            read_only_rootfs: Some(dc.read_only_rootfs),
            extra_caps_drop: vec![],
        })
    } else {
        None
    };

    let allow_network = match mode {
        SandboxMode::Sandboxed => !is_remote_session,
        _ => true,
    };

    // The jail denies whatever it is not told about; the toolchain/git grants
    // everyday commands need are resolved by TinyBox at spawn time
    // (`sandbox::local_grants`), so the policy starts without mounts.
    tracing::debug!(
        mode = ?mode,
        backend = ?backend,
        is_remote = is_remote_session,
        action_dir = %action_dir.display(),
        "[sandbox] resolved policy"
    );

    SandboxPolicy {
        backend,
        workspace_root: action_dir.to_path_buf(),
        state_dir: state_dir.to_path_buf(),
        read_only_mounts: Vec::new(),
        read_write_mounts: Vec::new(),
        allow_network,
        env_passthrough: SANDBOX_ENV_PASSTHROUGH
            .iter()
            .map(|s| s.to_string())
            .collect(),
        docker_overrides,
    }
}

/// Create a backend handle for the resolved policy. For Docker this
/// checks availability; for Local it checks the OS backend.
/// The status a `Local` sandbox handle reports, given the backend `pick_backend`
/// actually chose.
///
/// Extracted as a free function so the decision is testable on **any** host.
/// Asserting it through `create_sandbox_backend` cannot work: which branch runs
/// depends on whether the machine has an OS jail, so on a host with Seatbelt or
/// Landlock the noop path is never reached and a regression to "always Ready"
/// passes unnoticed. That is exactly how the original defect survived.
#[cfg(any(feature = "modules", test))]
pub(crate) fn local_status_for_jail(
    isolation: Option<super::types::SandboxIsolation>,
) -> SandboxStatus {
    if isolation.is_some() {
        SandboxStatus::Ready
    } else {
        SandboxStatus::Inactive
    }
}

/// The refusal every spawner reports when the security module planned the spawn
/// and refused it (the isolation floor, a denied grant, a human approval it
/// cannot get). A typed error so callers can tell a deterministic policy
/// refusal (never retried) from a transient failure such as a module fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IsolationFloor;

impl std::fmt::Display for IsolationFloor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the security module refused to plan this sandboxed spawn")
    }
}

impl std::error::Error for IsolationFloor {}

pub async fn create_sandbox_backend(
    ctx: &DispatchCtx,
    policy: &SandboxPolicy,
) -> SandboxBackendHandle {
    if policy.backend != SandboxBackendKind::Docker {
        return create_sandbox_backend_with_config(policy, &crate::config::Config::default())
            .await
            .unwrap_or(SandboxBackendHandle {
                kind: policy.backend,
                status: SandboxStatus::Error,
                backend_id: None,
                isolation: None,
            });
    }
    let config = match ctx.load_config().await {
        Ok(config) => config,
        Err(_) if policy.backend == SandboxBackendKind::Docker => {
            return docker_error_handle();
        }
        Err(_) => crate::config::Config::default(),
    };
    create_sandbox_backend_with_config(policy, &config)
        .await
        .unwrap_or_else(|_| docker_error_handle())
}

/// Resolve backend status with the caller's loaded configuration.
pub async fn create_sandbox_backend_with_config(
    policy: &SandboxPolicy,
    config: &crate::config::Config,
) -> Result<SandboxBackendHandle, String> {
    match policy.backend {
        SandboxBackendKind::None => Ok(SandboxBackendHandle {
            kind: SandboxBackendKind::None,
            status: SandboxStatus::Ready,
            backend_id: None,
            isolation: None,
        }),
        SandboxBackendKind::Local => {
            let (backend_name, status, isolation) = super::native_jail::status(config).await?;
            if status == SandboxStatus::Inactive {
                tracing::warn!(
                    backend = backend_name,
                    "[sandbox:local] TinyBox native jail is unavailable or below the required isolation floor"
                );
            }
            Ok(SandboxBackendHandle {
                kind: SandboxBackendKind::Local,
                status,
                backend_id: Some(backend_name),
                isolation,
            })
        }
        SandboxBackendKind::Docker => docker::docker_backend_handle(config).await,
    }
}

fn docker_error_handle() -> SandboxBackendHandle {
    SandboxBackendHandle {
        kind: SandboxBackendKind::Docker,
        status: SandboxStatus::Error,
        backend_id: None,
        isolation: None,
    }
}

/// Execute a command through the appropriate sandbox backend.
///
/// Returns the sandboxed execution result. The caller (typically the
/// shell tool) is responsible for converting this into a `ToolResult`.
pub async fn execute_in_sandbox(
    ctx: &DispatchCtx,
    policy: &SandboxPolicy,
    command: &str,
    working_dir: &Path,
    extra_env: HashMap<OsString, OsString>,
    timeout: Duration,
) -> anyhow::Result<SandboxExecResult> {
    execute_in_sandbox_metered(ctx, policy, command, working_dir, extra_env, timeout, None).await
}

/// [`execute_in_sandbox`] that feeds `meter` with the command's output as it
/// streams (the native backends; Docker reports elapsed time only), for the
/// shell tool's progress line.
pub(crate) async fn execute_in_sandbox_metered(
    ctx: &DispatchCtx,
    policy: &SandboxPolicy,
    command: &str,
    working_dir: &Path,
    extra_env: HashMap<OsString, OsString>,
    timeout: Duration,
    meter: Option<super::shell_progress::ShellOutputMeter>,
) -> anyhow::Result<SandboxExecResult> {
    // The saved backend is an operator's explicit isolation requirement. It
    // must be checked at the spawn boundary, including callers that resolved
    // their policy from an older or default RuntimeConfig.
    let mut effective_policy = policy.clone();
    let host_off = !crate::core::runtime::is_saas() && sandbox_disabled_by_host(ctx);
    if host_off {
        effective_policy.backend = SandboxBackendKind::None;
    }
    let mut module_config = None;
    if !(crate::core::runtime::is_saas() || sandbox_disabled_by_host(ctx)) {
        let config = ctx
            .load_config()
            .await
            .map_err(|e| anyhow::anyhow!("Cannot read sandbox configuration: {e}"))?;
        module_config = Some(config.clone());
        if !config_disables_sandbox(&config) {
            apply_requested_backend(
                &mut effective_policy,
                &config.sandbox,
                &config.runtime,
                &config,
            )
            .await?;
        }
    }
    let policy = &effective_policy;
    // SaaS never runs a command unisolated: a caller that resolved a policy with
    // no backend (an agent mode of `None`, a stale default) is refused here, at
    // the one spawn boundary, instead of falling through to the host shell.
    if crate::core::runtime::is_saas() && policy.backend == SandboxBackendKind::None {
        tracing::warn!("[sandbox] refusing an unsandboxed spawn under SaaS");
        anyhow::bail!(
            "SaaS commands must run in the user's sandbox; an unsandboxed spawn is refused"
        );
    }
    // Validate the working directory up front so a missing/bad action_dir
    // surfaces an actionable, path-naming error here rather than an opaque OS
    // error 267 (ERROR_DIRECTORY) at spawn time — parity with the unsandboxed
    // `NativeRuntime::build_shell_command` guard. (#3353, Fix 2)
    //
    // The validation is host-side, so it is applied per-backend: for None/Local
    // `working_dir` *is* a host path; for Docker `working_dir` is the
    // container-side mount target (e.g. `/workspace`) which must NOT be
    // stat'd/created on the host — there we validate the host-side mount source
    // (`policy.workspace_root`) instead.
    match policy.backend {
        SandboxBackendKind::None => {
            crate::config::ensure_usable_cwd(working_dir)?;
            let config = match module_config {
                Some(config) => config,
                None => ctx.load_config().await.map_err(|error| {
                    anyhow::anyhow!("Cannot read module configuration: {error}")
                })?,
            };
            execute_unsandboxed(
                &config,
                policy,
                command,
                working_dir,
                &extra_env,
                timeout,
                meter,
            )
            .await
        }
        SandboxBackendKind::Local => {
            crate::config::ensure_usable_cwd(working_dir)?;
            let config = match module_config {
                Some(config) => config,
                None => ctx.load_config().await.map_err(|error| {
                    anyhow::anyhow!("Cannot read module configuration: {error}")
                })?,
            };
            let policy = super::local_grants::with_resolved_grants(&config, policy).await?;
            let admitted =
                admit_spawn(&config, &policy, command, working_dir, &extra_env, host_off).await?;
            super::native_jail::execute(
                &config,
                &admitted.policy,
                command,
                working_dir,
                &extra_env,
                timeout,
                admitted.jail,
                false,
                meter,
            )
            .await
        }
        SandboxBackendKind::Docker => {
            crate::config::ensure_usable_cwd(&policy.workspace_root)?;
            let request = SandboxExecRequest {
                command: command.to_string(),
                working_dir: working_dir.to_path_buf(),
                env: extra_env,
                timeout,
            };
            let config = match module_config {
                Some(config) => config,
                None => ctx.load_config().await.map_err(|error| {
                    anyhow::anyhow!("Cannot read module configuration: {error}")
                })?,
            };
            let admitted = admit_spawn(
                &config,
                policy,
                command,
                working_dir,
                &request.env,
                host_off,
            )
            .await?;
            docker::docker_exec(&config, &admitted.policy, &request).await
        }
    }
}

/// One spawn the TinySecurity module planned and admitted: the policy narrowed to
/// the plan, and the jail the plan describes for TinyBox.
#[derive(Debug)]
pub(crate) struct Admitted {
    /// The caller's policy with mounts, network and container bounds from the plan.
    pub(crate) policy: SandboxPolicy,
    /// The native jail the plan maps to; the Local backend runs exactly this.
    pub(crate) jail: super::native_jail::PlannedJail,
}

/// Have the TinySecurity module plan and admit one sandboxed spawn, and return the
/// policy and jail it must run under. The module's plan, not the resolved policy,
/// bounds the mounts, the network and the container limits; a refusal or a module
/// fault is an error and the command does not run.
#[cfg(feature = "security-module")]
async fn admit_spawn(
    config: &crate::config::Config,
    policy: &SandboxPolicy,
    command: &str,
    working_dir: &Path,
    env: &HashMap<OsString, OsString>,
    host_off: bool,
) -> anyhow::Result<Admitted> {
    let env = env
        .iter()
        .map(|(name, value)| match (name.to_str(), value.to_str()) {
            (Some(name), Some(value)) => Ok((name.to_owned(), value.to_owned())),
            _ => Err(anyhow::anyhow!("sandbox environment is not UTF-8")),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let operation = super::spawn_plan::SpawnOperation {
        command,
        cwd: working_dir,
        env,
        stdin: None,
        preserve_pipeline_status: false,
    };
    admit_planned(super::spawn_plan::SpawnRequest {
        config,
        policy,
        source: super::spawn_plan::SpawnSource::Agent,
        operation: &operation,
        host_off,
        module: super::spawn_plan::ModuleSource::Production,
    })
    .await
}

/// Plan, reserve and map one spawn. A module refusal is the typed
/// [`IsolationFloor`] (deterministic, never retried); a fault stays a plain error.
#[cfg(feature = "security-module")]
pub(super) async fn admit_planned(
    request: super::spawn_plan::SpawnRequest<'_>,
) -> anyhow::Result<Admitted> {
    use super::spawn_plan::PlanError;
    let result = async {
        let planned = super::spawn_plan::plan(request).await?;
        super::spawn_plan::consume(&planned).await?;
        Ok::<_, PlanError>(Admitted {
            policy: planned.apply(request.policy),
            jail: planned.plan.jail,
        })
    }
    .await;
    result.map_err(|error| match error {
        PlanError::Refused(_) => anyhow::Error::new(IsolationFloor).context(error.to_string()),
        PlanError::Unavailable(_) => anyhow::Error::new(error),
    })
}

/// Without the module there is no plan to ask for, and nothing runs unplanned.
#[cfg(not(feature = "security-module"))]
async fn admit_spawn(
    _config: &crate::config::Config,
    _policy: &SandboxPolicy,
    _command: &str,
    _working_dir: &Path,
    _env: &HashMap<OsString, OsString>,
    _host_off: bool,
) -> anyhow::Result<Admitted> {
    anyhow::bail!("the security module is disabled; sandboxed spawns are refused")
}

/// Apply an operator-selected backend. Auto retains the caller's existing
/// policy. None opts out only when no agent-level sandbox is required.
pub(crate) async fn apply_requested_backend(
    policy: &mut SandboxPolicy,
    sandbox: &SandboxConfig,
    runtime: &RuntimeConfig,
    config: &crate::config::Config,
) -> anyhow::Result<()> {
    // `[runtime] kind = "docker"` is the legacy spelling of an operator-required
    // container; it selects the same module-planned Docker spawn.
    let docker_runtime = runtime.kind == "docker";
    match &sandbox.backend {
        SandboxBackend::Auto if !docker_runtime => Ok(()),
        SandboxBackend::None => Ok(()),
        SandboxBackend::Auto | SandboxBackend::Docker => {
            if docker_runtime && !runtime.docker.mount_workspace {
                // The module's plan always grants the workspace; honouring a
                // workspace-less container would need a plan the host cannot ask for.
                anyhow::bail!(
                    "[runtime.docker] mount_workspace = false is not supported: sandboxed spawns always grant the workspace"
                );
            }
            if policy.backend == SandboxBackendKind::Local {
                policy.read_only_mounts.clear();
                policy.read_write_mounts.clear();
            }
            policy.backend = SandboxBackendKind::Docker;
            let dc = &runtime.docker;
            policy.docker_overrides = Some(super::types::DockerOverrides {
                image: Some(dc.image.clone()),
                network: Some(dc.network.clone()),
                memory_limit_mb: dc.memory_limit_mb,
                cpu_limit: dc.cpu_limit,
                read_only_rootfs: Some(dc.read_only_rootfs),
                extra_caps_drop: vec![],
            });
            Ok(())
        }
        SandboxBackend::Landlock => {
            super::native_jail::require_backend(config, "landlock").await?;
            policy.backend = SandboxBackendKind::Local;
            policy.docker_overrides = None;
            Ok(())
        }
    }
}

/// Passthrough execution with no sandbox (for `SandboxBackendKind::None`).
///
/// TinyBox's `StartNativeShellExec` selects the shell (so Windows gets
/// `cmd.exe`, #4705), clears the environment down to its safe snapshot (the
/// [`SANDBOX_ENV_PASSTHROUGH`] names plus the Windows process-bootstrap
/// variables), applies the caller's overrides and owns the scratch directory.
async fn execute_unsandboxed(
    config: &crate::config::Config,
    policy: &SandboxPolicy,
    command: &str,
    working_dir: &Path,
    extra_env: &HashMap<OsString, OsString>,
    timeout: Duration,
    meter: Option<super::shell_progress::ShellOutputMeter>,
) -> anyhow::Result<SandboxExecResult> {
    let mut env = std::collections::BTreeMap::new();
    for (name, value) in extra_env {
        let (Some(name), Some(value)) = (name.to_str(), value.to_str()) else {
            anyhow::bail!("sandbox environment is not UTF-8");
        };
        env.insert(name.to_owned(), value.to_owned());
    }
    std::fs::create_dir_all(&policy.state_dir)
        .map_err(|error| anyhow::anyhow!("Cannot prepare sandbox state directory: {error}"))?;
    super::shell_exec::run_host(
        config,
        command,
        working_dir,
        &policy.state_dir,
        env,
        timeout,
        false,
        meter,
    )
    .await
}

/// Check whether a tool operation is an elevated op that must run on the
/// host even when the session is sandboxed.
pub fn is_elevated_op(tool_name: &str) -> bool {
    ELEVATED_TOOLS.contains(&tool_name)
}

/// Build an `ElevatedOp` for audit logging when a tool bypasses the sandbox.
pub fn build_elevated_op(tool_name: &str, command: &str, reason: &str) -> ElevatedOp {
    tracing::info!(
        tool = tool_name,
        reason = reason,
        "[sandbox] elevated host operation"
    );
    ElevatedOp {
        tool_name: tool_name.to_string(),
        reason: reason.to_string(),
        command: command.to_string(),
    }
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
