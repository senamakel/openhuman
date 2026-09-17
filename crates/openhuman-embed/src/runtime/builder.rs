//! Assembling a [`Runtime`].
//!
//! The builder turns typed inputs into one in-memory base [`Config`] plus a
//! [`DomainSet`]/[`ServiceSet`]/[`ToolGroups`] triple, installs the API key
//! into the credential store, and hands everything to [`CoreBuilder`].
//! Nothing here mutates the process environment.

use std::sync::Arc;

use openhuman_core::config::Config;
use openhuman_core::core::runtime::{CoreBuilder, DomainSet, ServiceSet, TokenSource};
use openhuman_core::core::types::HostKind;
use openhuman_core::tools::toolpacks::ToolGroups;

use super::{ApiKey, Runtime, RuntimeError, RUNTIME_LIVE};
use crate::harness::workspace::ResolvedWorkspace;
use crate::harness::{Access, Provider, Workspace};
use crate::{Core, Session};

/// Builder for a [`Runtime`]. Obtain with [`Runtime::builder`].
pub struct RuntimeBuilder {
    workspace: Workspace,
    provider: Provider,
    access: Access,
    services: Option<ServiceSet>,
    domains: Option<DomainSet>,
    tool_groups: Option<ToolGroups>,
    host_kind: HostKind,
    config: Option<Config>,
    session: Option<Session>,
    backend_url: Option<String>,
    api_key: Option<ApiKey>,
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeBuilder {
    /// A builder with safe defaults: an ephemeral workspace, the machine's
    /// configured inference, the supervised access tier, no background
    /// services, and every domain a library agent can use.
    pub fn new() -> Self {
        Self {
            workspace: Workspace::default(),
            provider: Provider::inherit(),
            access: Access::default(),
            services: None,
            domains: None,
            tool_groups: None,
            host_kind: HostKind::Library,
            config: None,
            session: None,
            backend_url: None,
            api_key: None,
        }
    }

    /// Where the runtime keeps its credential store, session database and
    /// every agent's memory, transcripts and skills.
    pub fn workspace(mut self, workspace: Workspace) -> Self {
        self.workspace = workspace;
        self
    }

    /// The TinyHumans API key — the runtime's only credential.
    ///
    /// Installed into the runtime's credential store before the core boots.
    /// Managed inference then sends it as a bearer to the TinyHumans
    /// OpenAI-compatible endpoint and backend REST calls send it as
    /// `x-api-key`; no user session is involved. Agents that name their own
    /// [`Provider`] never use it.
    pub fn api_key(mut self, key: impl Into<ApiKey>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Point the core's backend calls at `url`.
    ///
    /// Unset means whatever [`Config`] resolves to — for a fresh config the
    /// hosted TinyHumans backend. Set it to a stub or a self-hosted backend
    /// when the runtime should not reach the real one.
    pub fn backend_url(mut self, url: impl Into<String>) -> Self {
        self.backend_url = Some(url.into());
        self
    }

    /// Default provider for agents that do not name one.
    pub fn provider(mut self, provider: Provider) -> Self {
        self.provider = provider;
        self
    }

    /// Default access tier for agents that do not name one. Defaults to
    /// [`Access::supervised`].
    pub fn access(mut self, access: Access) -> Self {
        self.access = access;
        self
    }

    /// Override which background services run.
    ///
    /// The default is deliberately minimal (only the harness init step):
    /// cron, heartbeat and the memory queue each write to the workspace on
    /// their own schedule, turning a library call into a background process
    /// the caller did not ask for.
    pub fn services(mut self, services: ServiceSet) -> Self {
        self.services = Some(services);
        self
    }

    /// Override which domain families exist at runtime.
    ///
    /// Families are registered once, at boot, so an agent can only *narrow*
    /// this set. The default is [`DomainSet::embedded`] plus `mcp` and
    /// `skills` when those features are compiled in — the runtime cannot know
    /// yet which agents will declare servers or skills, and an agent that
    /// declares none narrows them back off for itself.
    pub fn domains(mut self, domains: DomainSet) -> Self {
        self.domains = Some(domains);
        self
    }

    /// Default tool-group disclosure. Agents may narrow it.
    ///
    /// Defaults to every group withheld behind `use_skill`, matching the
    /// desktop app. See [`ToolGroups::advertised`] and [`ToolGroups::none`].
    pub fn tool_groups(mut self, tool_groups: ToolGroups) -> Self {
        self.tool_groups = Some(tool_groups);
        self
    }

    /// Identify the host to the core. Defaults to [`HostKind::Library`],
    /// which accepts caller-supplied provider credentials without an
    /// OpenHuman app login.
    pub fn host_kind(mut self, host_kind: HostKind) -> Self {
        self.host_kind = host_kind;
        self
    }

    /// Install an app session before the first turn.
    ///
    /// Kept for hosts that drive authenticated backend features on behalf of
    /// a signed-in user. A runtime with an [`api_key`](Self::api_key) does not
    /// need one.
    pub fn session(mut self, session: Session) -> Self {
        self.session = Some(session);
        self
    }

    /// Start from a caller-supplied [`Config`] instead of the default.
    ///
    /// Every other builder method is applied **on top** of it, and every
    /// agent starts from the result, so this is the escape hatch for the
    /// config fields the builder does not model — not a way to bypass them.
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Build the core and return a runtime ready to host agents.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::AlreadyRunning`] if this process already has one;
    /// [`RuntimeError::BlankApiKey`] for an empty key.
    pub async fn build(self) -> Result<Runtime, RuntimeError> {
        // Claim the process slot before doing any work, so a losing racer
        // neither creates a temp dir nor half-initializes global state.
        if RUNTIME_LIVE.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return Err(RuntimeError::AlreadyRunning);
        }
        // From here on every early return must release the slot, or a failed
        // build would permanently poison the process against retrying.
        match self.build_inner().await {
            Ok(runtime) => Ok(runtime),
            Err(e) => {
                RUNTIME_LIVE.store(false, std::sync::atomic::Ordering::Release);
                Err(e)
            }
        }
    }

    async fn build_inner(self) -> Result<Runtime, RuntimeError> {
        if self.api_key.as_ref().is_some_and(ApiKey::is_blank) {
            return Err(RuntimeError::BlankApiKey);
        }
        let inherit = self.workspace.is_operator_owned();
        let resolved = ResolvedWorkspace::resolve(&self.workspace, None).map_err(map_ws)?;

        // `Inherit` starts from the operator's own config — loaded here rather
        // than left to `build()` to discover, because the builder's other
        // knobs (access tier, backend URL, API key) are applied *on top* of it.
        let mut config = match (&self.workspace, self.config) {
            (Workspace::Inherit, Some(config)) => config,
            (Workspace::Inherit, None) => {
                Config::load_or_init().await.map_err(RuntimeError::Build)?
            }
            (_, supplied) => {
                let mut config = supplied.unwrap_or_default();
                config.workspace_dir = resolved.workspace_dir.clone();
                config.action_dir = resolved.action_dir.clone();
                // Credential state, auth profiles and the keyring file backend
                // all resolve against this path's parent, not `workspace_dir`.
                config.config_path = resolved.config_path.clone();
                config
            }
        };

        if let Some(url) = self.backend_url.clone() {
            config.api_url = Some(url);
        }
        self.access.apply(&mut config);
        apply_provider(&mut config, &self.provider);

        // Before `CoreBuilder::build()`: the scheduler gate reads the credential
        // store exactly once, at boot, to decide whether it is signed in.
        let has_api_key = if let Some(key) = self.api_key.as_ref() {
            let state_dir = config
                .config_path
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| config.config_path.clone());
            openhuman_core::security::credentials::api_key::store_api_key_in(
                &state_dir,
                config.secrets.encrypt,
                key.expose(),
            )
            .map_err(RuntimeError::Build)?;
            true
        } else {
            false
        };

        // An endpoint without a model is deliberately ignored by the route
        // applicator, so host policy follows the effective behaviour: only a
        // *usable* runtime-default route exempts an inherited install from
        // its session gate. An API key is a credential in its own right.
        let routed_provider_effective = self.provider.has_usable_route()
            && config
                .default_model
                .as_deref()
                .is_some_and(|model| !model.trim().is_empty());
        let host_kind = effective_host_kind(
            self.host_kind,
            inherit,
            routed_provider_effective || has_api_key,
        );

        let domains = self.domains.unwrap_or_else(default_domains);
        let tool_groups = self.tool_groups.unwrap_or_default();
        let services = self.services.unwrap_or_else(default_services);

        log::debug!(
            "[embed][runtime] building host_kind={:?} inherit_workspace={inherit} \
             routed_provider={} api_key={has_api_key} domains={domains:?} tool_groups={tool_groups:?}",
            host_kind,
            self.provider.is_routed(),
        );

        let runtime = CoreBuilder::new(host_kind)
            .domains(domains)
            .tool_groups(tool_groups.clone())
            .services(services)
            .token(TokenSource::EnvOrFile)
            .config(config.clone())
            .build()
            .await
            .map_err(RuntimeError::Build)?;
        let core = Core::from_runtime(Arc::new(runtime));

        if let Some(session) = self.session {
            core.auth().store(session).await?;
        }

        Ok(Runtime::new(
            core,
            resolved,
            config,
            inherit,
            domains,
            tool_groups,
            self.provider,
            self.access,
        ))
    }
}

fn map_ws(err: crate::HarnessError) -> RuntimeError {
    match err {
        crate::HarnessError::Workspace { what, source } => RuntimeError::Workspace { what, source },
        crate::HarnessError::Invalid(msg) => RuntimeError::Invalid(msg),
        crate::HarnessError::Build(e) => RuntimeError::Build(e),
        crate::HarnessError::Call(e) => RuntimeError::Call(e),
        crate::HarnessError::AlreadyRunning => RuntimeError::AlreadyRunning,
    }
}

/// Preserve the installed application's authentication policy when the
/// runtime borrows both its workspace and provider. `Library` means the host
/// supplied inference (a route or an API key); it must not become a blanket
/// way to bypass the session gate around an operator-installed provider.
pub(crate) fn effective_host_kind(
    requested: HostKind,
    inherit_workspace: bool,
    host_supplied_credential: bool,
) -> HostKind {
    if requested == HostKind::Library && inherit_workspace && !host_supplied_credential {
        HostKind::Cli
    } else {
        requested
    }
}

/// Apply a [`Provider`]'s model to the config.
///
/// Only the model: the *route* is a per-turn parameter, never a config write,
/// because config routes persist. See the [`provider`](crate::harness::provider)
/// module docs.
pub(crate) fn apply_provider(config: &mut Config, provider: &Provider) {
    if let Some(model) = provider.model_id() {
        config.default_model = Some(model.to_string());
    }
}

/// Domain families a runtime registers by default: the embedded set plus
/// whichever of `mcp` / `skills` this build compiles in, because agents can
/// only narrow what the runtime registered.
pub(crate) fn default_domains() -> DomainSet {
    #[allow(unused_mut)]
    let mut domains = DomainSet::embedded();
    #[cfg(feature = "mcp")]
    {
        domains.mcp = true;
    }
    #[cfg(feature = "skills")]
    {
        domains.skills = true;
    }
    domains
}

/// Background services a runtime runs by default: only `harness_init`.
pub(crate) fn default_services() -> ServiceSet {
    ServiceSet {
        harness_init: true,
        ..ServiceSet::none()
    }
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
