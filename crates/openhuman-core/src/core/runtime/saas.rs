//! Booting a core in [`Mode::Saas`](super::mode::Mode::Saas).
//!
//! A SaaS core serves many users from one process, each as their own agent,
//! behind a trusted gateway that authenticates them. This module holds the
//! **operator** side of that: [`SaasConfig`] (read from the operator's file,
//! never from any user's `config.toml`), the SaaS presets for the three
//! narrowing axes, and [`build`], which refuses to boot unless
//! [`boot_guard`](super::boot_guard) finds nothing unsafe.
//!
//! The per-user surface lands phase by phase. Until then the presets are
//! closed: [`DomainSet::saas`] enables only the operator plane
//! (`user_agents.*`), so a SaaS core answers its always-on infrastructure
//! (`core.*`, `/health`, `/schema`) and provisioning, and refuses every user
//! domain method as unknown. [`build`] installs the process's
//! [`AgentHost`](crate::user_agents::AgentHost).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use super::boot_guard::{self, BootInputs, ServiceToken};
use super::mode::{self, Mode};
use super::{CoreBuilder, CoreContext, CoreRuntime, DomainSet, ServiceSet, TokenSource};
use crate::core::types::HostKind;
use crate::tools::toolpacks::ToolGroups;

/// The operator's SaaS deployment settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaasConfig {
    /// Root of every user's state and the operator's own. Must be an absolute,
    /// existing directory that is not world-writable.
    pub root: PathBuf,
    /// The gateway's bearer, one line in a `0600` file. Defaults to
    /// `<root>/service.token`.
    #[serde(default)]
    pub service_token_file: Option<PathBuf>,
    /// Tool groups the operator opts back in. Refused by the boot guard until
    /// per-user sandboxing ships.
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    /// Extra RPC methods the operator exposes. Refused by the boot guard until
    /// the per-user RPC surface ships.
    #[serde(default)]
    pub rpc_allowlist_extra: Vec<String>,
    /// Most user agents kept open at once.
    #[serde(default = "default_max_agents_open")]
    pub max_agents_open: usize,
    /// Seconds an idle user agent stays open.
    #[serde(default = "default_idle_evict_secs")]
    pub idle_evict_secs: u64,
    /// Let every user ride the operator's backend API key.
    #[serde(default)]
    pub shared_backend_api_key: bool,
    /// Let users store their own agent definitions.
    #[serde(default)]
    pub custom_definitions: bool,
    /// Require `X-OpenHuman-User-Sig` on every request made for a user
    /// (see `user_agents::gateway`).
    #[serde(default = "default_true")]
    pub require_user_signature: bool,
}

fn default_true() -> bool {
    true
}

fn default_max_agents_open() -> usize {
    256
}

fn default_idle_evict_secs() -> u64 {
    30 * 60
}

impl SaasConfig {
    /// A config rooted at `root` with every default.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            service_token_file: None,
            tool_allowlist: Vec::new(),
            rpc_allowlist_extra: Vec::new(),
            max_agents_open: default_max_agents_open(),
            idle_evict_secs: default_idle_evict_secs(),
            shared_backend_api_key: false,
            custom_definitions: false,
            require_user_signature: true,
        }
    }

    /// Read the operator's TOML file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading SaaS config {}: {e}", path.display()))?;
        let config: Self = toml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("parsing SaaS config {}: {e}", path.display()))?;
        log::debug!(
            "[saas] loaded operator config from {} (root={})",
            path.display(),
            config.root.display()
        );
        Ok(config)
    }

    /// Where the gateway bearer is read from.
    pub fn service_token_path(&self) -> PathBuf {
        self.service_token_file
            .clone()
            .unwrap_or_else(|| self.root.join("service.token"))
    }

    /// The operator's own state directory: `<root>/operator`.
    pub fn operator_dir(&self) -> PathBuf {
        self.root.join("operator")
    }

    /// The config the operator plane boots with. It roots every path under
    /// [`Self::operator_dir`], so booting never resolves the host's
    /// `~/.openhuman` or an `active_user.toml`.
    pub fn operator_config(&self) -> crate::config::Config {
        let dir = self.operator_dir();
        crate::config::Config {
            workspace_dir: dir.join("workspace"),
            config_path: dir.join("config.toml"),
            action_dir: dir.join("action"),
            ..crate::config::Config::default()
        }
    }
}

impl ServiceSet {
    /// The services a SaaS core runs: the HTTP JSON-RPC transport and nothing
    /// that acts on a user's data from the background.
    pub fn saas() -> Self {
        Self::headless_api()
    }
}

impl DomainSet {
    /// The domain families a SaaS core registers: the operator plane and the
    /// user families whose per-user isolation has landed. User agents derive
    /// their contexts from these; `user_agents::surface` keeps the operator
    /// scope on its own plane and each user on the user allowlist.
    pub fn saas() -> Self {
        Self {
            operator: true,
            threads: true,
            channels: true,
            memory: true,
            ..Self::none()
        }
    }
}

/// Boot a SaaS core: check the deployment, lock the process to SaaS, and
/// build the operator plane.
pub async fn build(
    config: SaasConfig,
    host: Option<String>,
    port: Option<u16>,
) -> anyhow::Result<CoreRuntime> {
    if CoreContext::current().is_some() {
        anyhow::bail!(
            "[saas] a core is already running in this process; a SaaS core must be the only one"
        );
    }
    mode::reserve_saas_boot().map_err(|e| anyhow::anyhow!("[saas] {e}"))?;

    let services = ServiceSet::saas();
    let domains = DomainSet::saas();
    let token = ServiceToken::read(&config.service_token_path());
    let env: Vec<(String, String)> = std::env::vars().collect();
    boot_guard::check(&BootInputs {
        host_kind: HostKind::Saas,
        services,
        domains,
        config: &config,
        token: &token,
        env: &env,
        home: dirs::home_dir(),
    })?;
    let ServiceToken::Valid(bearer) = token else {
        unreachable!("boot guard accepts only a valid service token");
    };

    mode::lock_mode(Mode::Saas).map_err(|e| anyhow::anyhow!("[saas] {e}"))?;

    let operator = config.operator_config();
    for dir in [&operator.workspace_dir, &operator.action_dir] {
        std::fs::create_dir_all(dir)
            .map_err(|e| anyhow::anyhow!("[saas] creating {}: {e}", dir.display()))?;
    }
    // Root the keyring (the master key and every stored credential) under the
    // operator directory, never the host's `~/.openhuman`.
    crate::security::keyring::init_workspace(&operator.workspace_dir);
    let keyring_dir = crate::security::keyring::store::workspace_dir_for_file_backend();
    if keyring_dir != operator.workspace_dir {
        anyhow::bail!(
            "[saas] the keyring is already rooted at {}; a SaaS core keeps it under {}",
            keyring_dir.display(),
            operator.workspace_dir.display()
        );
    }
    log::info!(
        "[saas] booting operator plane root={} max_agents_open={} idle_evict_secs={}",
        config.root.display(),
        config.max_agents_open,
        config.idle_evict_secs
    );

    let mut builder = CoreBuilder::new(HostKind::Saas)
        .token(TokenSource::Fixed(Arc::new(bearer)))
        .services(services)
        .domains(domains)
        .tool_groups(ToolGroups::none())
        .config(operator);
    if let Some(host) = host {
        builder = builder.host(host);
    }
    if let Some(port) = port {
        builder = builder.port(port);
    }
    let runtime = builder.build().await?;
    let host = Arc::new(crate::user_agents::AgentHost::new(
        config,
        runtime.context().clone(),
    ));
    crate::user_agents::host::install(Arc::clone(&host));
    crate::user_agents::background::spawn(host);
    Ok(runtime)
}

#[cfg(test)]
#[path = "saas_tests.rs"]
mod tests;
