//! Host adapter for the browser members of the loadable TinyComputer module.
//!
//! TinyComputer serves its browser members (`BrowserOpenSession`,
//! `BrowserNavigate`, ...) on the same interface as its desktop members, so
//! this adapter shares the desktop adapter's proxy, which loads the module and
//! refreshes its private configuration before a call.

use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};
use tinycomputer_bus::agent::Capabilities;
use tinycomputer_bus::browser::{
    names, Action, ActionOutcome, DownloadInfo, DownloadWaitRequest, NavigateRequest, PageState,
    PageText, ReadRequest, SessionId, SessionInfo, SessionOptions, SessionRef, SessionRequest,
    Snapshot, SnapshotRequest, Viewport,
};
use tinycomputer_bus::DesktopResponse;

use crate::config::Config;

pub const MODULE_ID: &str = crate::modules::desktop::MODULE_ID;
use super::chrome_hints::chrome_hint;

#[derive(Debug, thiserror::Error)]
pub enum BrowserCallError {
    #[error("TinyComputer browser unavailable: {0}")]
    Unavailable(String),
    #[error("browser request blocked: {0}")]
    Policy(String),
    #[error("{name}: {message}")]
    Bus { name: String, message: String },
}

impl BrowserCallError {
    fn from_bus(error: tinybus::Error) -> Self {
        Self::Bus {
            name: error.wire_name().to_owned(),
            message: error.to_string(),
        }
    }

    /// A failed reply names its error in `details.name` (the
    /// `ai.tinyhumans.tinycomputer.Browser.Error.*` spelling); older replies
    /// carry only the envelope code.
    fn from_response(response: &DesktopResponse) -> Self {
        let Some(error) = response.error.as_ref() else {
            return Self::Bus {
                name: "ModuleFailed".into(),
                message: format!("{} failed without an error", response.command),
            };
        };
        let name = error
            .details
            .as_ref()
            .and_then(|details| details.get("name"))
            .and_then(serde_json::Value::as_str)
            .map(|name| name.rsplit('.').next().unwrap_or(name).to_owned())
            .unwrap_or_else(|| error.code.clone());
        let message = match chrome_hint(&error.message) {
            Some(hint) => format!("{} {hint}", error.message),
            None => error.message.clone(),
        };
        Self::Bus { name, message }
    }
}

/// A TinyComputer browser connection that applies this host's settings and shared web policy.
#[derive(Clone)]
pub struct BrowserClient {
    config: Arc<Config>,
}

impl BrowserClient {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub async fn ensure_ready(&self) -> Result<(), BrowserCallError> {
        let version = self.contract_version().await?;
        if !tinycomputer_bus::is_compatible(version) {
            return Err(BrowserCallError::Unavailable(format!(
                "incompatible TinyComputer contract: module {version:?}, host {:?}",
                tinycomputer_bus::CONTRACT_VERSION
            )));
        }
        Ok(())
    }

    /// The module's contract version, from `Describe` (which answers raw
    /// [`Capabilities`], not an envelope).
    pub async fn contract_version(&self) -> Result<(u32, u32), BrowserCallError> {
        let proxy = crate::modules::desktop::proxy(&self.config)
            .await
            .map_err(BrowserCallError::Unavailable)?;
        let capabilities: Capabilities = proxy
            .call(tinycomputer_bus::agent::names::methods::DESCRIBE, ())
            .await
            .map_err(BrowserCallError::from_bus)?;
        Ok(capabilities.contract_version)
    }

    pub async fn open_session(
        &self,
        options: SessionOptions,
    ) -> Result<SessionInfo, BrowserCallError> {
        self.open_session_with_origin(options, None).await
    }

    async fn open_session_with_origin(
        &self,
        mut options: SessionOptions,
        origin: Option<String>,
    ) -> Result<SessionInfo, BrowserCallError> {
        self.ensure_ready().await?;
        super::policy::validate_session_settings(self)?;
        let domains = super::policy::browser_domains(self);
        if domains.is_empty() && !super::policy::browser_allow_all() && !super::policy::allows_any_site(self) {
            return Err(BrowserCallError::Policy(
                "allowed websites list is empty".into(),
            ));
        }
        let browser = &self.config.browser;
        options.headless = browser.headless;
        options.viewport = Viewport::desktop(browser.viewport_width, browser.viewport_height);
        options.executable = browser.chrome_path.clone();
        options.user_data_dir = if browser.profile_mode == "persistent" {
            browser.profile_path.clone()
        } else {
            None
        };
        let claimed = claim_profile(
            &self.config,
            &mut options,
            crate::modules::profile_claim::SESSION_WAIT_MS,
        )
        .await;
        options.download_dir = browser.download_dir.clone();
        options.default_timeout_ms = browser.task_timeout_secs.saturating_mul(1000);
        options.allowed_origins =
            origin.map_or_else(|| super::policy::browser_origins(self), |origin| vec![origin]);
        let opened = self.call(names::methods::OPEN_SESSION, options).await;
        // An open session's Chrome holds the profile itself; one that did
        // not open holds nothing.
        if let Some(profile) = claimed {
            crate::modules::profile_claim::release(&self.config, &profile).await;
        }
        opened
    }

    /// Open a session bound to the host tree of an explicit destination.
    pub async fn open_session_for_url(&self, url: &str) -> Result<SessionInfo, BrowserCallError> {
        let origin = super::policy::explicit_origin(self, url)?;
        self.open_session_with_origin(SessionOptions::default(), origin)
            .await
    }

    async fn call<A: Serialize + Send, R: DeserializeOwned>(
        &self,
        member: &str,
        request: A,
    ) -> Result<R, BrowserCallError> {
        let proxy = crate::modules::desktop::proxy(&self.config)
            .await
            .map_err(BrowserCallError::Unavailable)?;
        tracing::debug!(member, "[browser] tinycomputer call");
        let response: serde_json::Value = proxy
            .call(member, (request,))
            .await
            .map_err(BrowserCallError::from_bus)?;
        decode_response(member, response)
    }
}

/// Unwraps the response for the browser call path. Kept separate so envelope
/// behavior can be exercised without loading the native module.
fn decode_response<R: DeserializeOwned>(
    member: &str,
    raw: serde_json::Value,
) -> Result<R, BrowserCallError> {
    let response: DesktopResponse =
        serde_json::from_value(raw.clone()).map_err(|error| BrowserCallError::Bus {
            name: "InvalidReply".into(),
            message: format!("{member} returned an unexpected reply: {error}"),
        })?;
    let data = match crate::modules::envelope::decode(
        Some(&response.version),
        Some(tinycomputer_bus::ENVELOPE_VERSION),
        response.ok,
        raw.get("data").cloned(),
        response.error.clone(),
    ) {
        Ok(data) => data,
        Err(crate::modules::envelope::EnvelopeError::Operation(_)) => {
            let error = BrowserCallError::from_response(&response);
            tracing::debug!(member, error = %error, "[browser] tinycomputer call failed");
            return Err(error);
        }
        Err(crate::modules::envelope::EnvelopeError::Malformed) => {
            return Err(BrowserCallError::Bus {
                name: "InvalidReply".into(),
                message: format!("{member} returned a malformed TinyComputer response envelope"),
            });
        }
    };
    serde_json::from_value(data).map_err(|error| BrowserCallError::Bus {
        name: "InvalidReply".into(),
        message: format!("{member} returned an unexpected reply: {error}"),
    })
}

fn session_ref(session: &SessionId) -> SessionRef {
    SessionRef {
        session: session.clone(),
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
