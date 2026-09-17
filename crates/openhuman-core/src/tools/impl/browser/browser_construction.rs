//! [`BrowserTool`] construction and availability probes.

use super::*;

impl BrowserTool {
    pub fn new(
        security: Arc<SecurityPolicy>,
        allowed_domains: Vec<String>,
        session_name: Option<String>,
    ) -> Self {
        Self::new_with_backend(
            security,
            allowed_domains,
            session_name,
            "agent_browser".into(),
            true,
            "http://127.0.0.1:9515".into(),
            None,
            ComputerUseConfig::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_backend(
        security: Arc<SecurityPolicy>,
        allowed_domains: Vec<String>,
        session_name: Option<String>,
        backend: String,
        native_headless: bool,
        native_webdriver_url: String,
        native_chrome_path: Option<String>,
        computer_use: ComputerUseConfig,
    ) -> Self {
        Self {
            security,
            allowed_domains: normalize_domains(allowed_domains),
            session_name,
            backend,
            native_headless,
            native_webdriver_url,
            native_chrome_path,
            computer_use,
            playwright_state: tokio::sync::Mutex::new(
                playwright_backend::PlaywrightBrowserState::default(),
            ),
            #[cfg(feature = "browser-native")]
            native_state: tokio::sync::Mutex::new(native_backend::NativeBrowserState::default()),
        }
    }

    /// Check if agent-browser tool is available
    pub async fn is_agent_browser_available() -> bool {
        Command::new("agent-browser")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Backward-compatible alias.
    pub async fn is_available() -> bool {
        Self::is_agent_browser_available().await
    }

    /// Check if the local Playwright package is available to Node.js.
    pub async fn is_playwright_available() -> bool {
        playwright_backend::PlaywrightBrowserState::is_available().await
    }

    pub(super) fn configured_backend(&self) -> anyhow::Result<BrowserBackendKind> {
        BrowserBackendKind::parse(&self.backend)
    }

    pub(crate) fn rust_native_compiled() -> bool {
        cfg!(feature = "browser-native")
    }

    pub(crate) fn rust_native_available(&self) -> bool {
        #[cfg(feature = "browser-native")]
        {
            native_backend::NativeBrowserState::is_available(
                self.native_headless,
                &self.native_webdriver_url,
                self.native_chrome_path.as_deref(),
            )
        }
        #[cfg(not(feature = "browser-native"))]
        {
            false
        }
    }
}
