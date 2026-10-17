//! Shared website, origin, and URL safety policy for browser calls.

use super::{BrowserCallError, BrowserClient};
use tinycomputer_bus::browser::SessionOptions;

pub(super) fn parsed_browser_url(raw: &str) -> Result<url::Url, BrowserCallError> {
    url::Url::parse(raw)
        .or_else(|_| url::Url::parse(&format!("https://{raw}")))
        .map_err(|_| BrowserCallError::Policy("invalid URL".into()))
}

pub(super) fn browser_allow_all() -> bool {
    matches!(
        std::env::var("OPENHUMAN_BROWSER_ALLOW_ALL").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

pub(super) fn private_or_local(host: &str) -> bool {
    // Preserve browser policy for multiple trailing dots before using the
    // canonical shared classifier. Browser URL parsing can retain these.
    tinytools_std::url_guard::is_private_or_local_host(
        &host.trim_end_matches('.').to_ascii_lowercase(),
    )
}

impl BrowserClient {
    pub(crate) fn explicit_origin(&self, raw: &str) -> Result<Option<String>, BrowserCallError> {
        self.explicit_origin_for_mode(raw, super::policy::browser_allow_all())
    }

    pub(super) fn explicit_origin_for_mode(
        &self,
        raw: &str,
        allow_all: bool,
    ) -> Result<Option<String>, BrowserCallError> {
        check_url_for_mode(self, raw, allow_all)?;
        if !allow_all {
            return Ok(None);
        }
        let url = super::policy::parsed_browser_url(raw)?;
        let host = url.host().expect("validated URL has a host");
        Ok(Some(match host {
            url::Host::Domain(host) => format!("https://.{}", host.trim_end_matches('.')),
            url::Host::Ipv4(_) | url::Host::Ipv6(_) => url.origin().ascii_serialization(),
        }))
    }

    pub async fn navigate(
        &self,
        session: &SessionId,
        request: NavigateRequest,
    ) -> Result<PageState, BrowserCallError> {
        check_url(self, &request.url)?;
        let page: PageState = self
            .call(
                names::methods::NAVIGATE,
                SessionRequest::new(session.clone(), request),
            )
            .await?;
        check_returned_url(self, &page.url)?;
        Ok(page)
    }

    pub async fn snapshot(
        &self,
        session: &SessionId,
        request: SnapshotRequest,
    ) -> Result<Snapshot, BrowserCallError> {
        let snapshot: Snapshot = self
            .call(
                names::methods::SNAPSHOT,
                SessionRequest::new(session.clone(), request),
            )
            .await?;
        check_returned_url(self, &snapshot.url)?;
        Ok(snapshot)
    }

    pub async fn perform(
        &self,
        session: &SessionId,
        action: Action,
    ) -> Result<ActionOutcome, BrowserCallError> {
        let outcome: ActionOutcome = self
            .call(
                names::methods::PERFORM,
                SessionRequest::new(session.clone(), action),
            )
            .await?;
        check_returned_url(self, &outcome.page.url)?;
        Ok(outcome)
    }

    pub async fn read_page(
        &self,
        session: &SessionId,
        request: ReadRequest,
    ) -> Result<PageText, BrowserCallError> {
        let page: PageText = self
            .call(
                names::methods::READ_PAGE,
                SessionRequest::new(session.clone(), request),
            )
            .await?;
        check_returned_url(self, &page.url)?;
        Ok(page)
    }

    pub async fn list_downloads(
        &self,
        session: &SessionId,
    ) -> Result<Vec<DownloadInfo>, BrowserCallError> {
        self.call(names::methods::LIST_DOWNLOADS, session_ref(session))
            .await
    }

    pub async fn wait_download(
        &self,
        session: &SessionId,
        request: DownloadWaitRequest,
    ) -> Result<DownloadInfo, BrowserCallError> {
        self.call(
            names::methods::WAIT_DOWNLOAD,
            SessionRequest::new(session.clone(), request),
        )
        .await
    }

    pub async fn close_session(&self, session: &SessionId) -> Result<(), BrowserCallError> {
        let _closed: serde_json::Value = self
            .call(names::methods::CLOSE_SESSION, session_ref(session))
            .await?;
        Ok(())
    }

    pub(crate) fn check_url(&self, raw: &str) -> Result<(), BrowserCallError> {
        check_url_for_mode(self, raw, super::policy::browser_allow_all())
    }

    pub(super) fn check_url_for_mode(&self, raw: &str, allow_all: bool) -> Result<(), BrowserCallError> {
        if raw.trim().is_empty() || raw.chars().any(char::is_whitespace) {
            return Err(BrowserCallError::Policy("invalid URL".into()));
        }
        let url = super::policy::parsed_browser_url(raw)?;
        if url.scheme() != "https" {
            return Err(BrowserCallError::Policy(
                "only HTTPS URLs are allowed".into(),
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(BrowserCallError::Policy(
                "URL userinfo is not allowed".into(),
            ));
        }
        let host = url
            .host_str()
            .ok_or_else(|| BrowserCallError::Policy("URL has no host".into()))?;
        let is_private = match url.host() {
            Some(url::Host::Ipv4(ip)) => super::policy::private_or_local(&ip.to_string()),
            Some(url::Host::Ipv6(ip)) => super::policy::private_or_local(&ip.to_string()),
            _ => super::policy::private_or_local(host),
        };
        if is_private {
            return Err(BrowserCallError::Policy(format!(
                "local/private host '{host}' is blocked"
            )));
        }
        if !allow_all
            && !allows_any_site(self)
            && !browser_domains(self).iter().any(|allowed| {
                host.eq_ignore_ascii_case(allowed)
                    || host
                        .to_ascii_lowercase()
                        .ends_with(&format!(".{}", allowed.to_ascii_lowercase()))
            })
        {
            return Err(BrowserCallError::Policy(format!(
                "host '{host}' is not in allowed websites"
            )));
        }
        Ok(())
    }

    pub(super) fn check_returned_url(&self, raw: &str) -> Result<(), BrowserCallError> {
        // A fresh session starts at about:blank. It is internal to Chrome and
        // does not make a network request; reporting it lets the agent see it
        // needs to navigate before inspecting page content.
        if raw.eq_ignore_ascii_case("about:blank") {
            return Ok(());
        }
        check_url(self, raw)
    }

    /// Whether the allowed websites are "Allow all" (`*`): the browser may
    /// open any public site, as web fetch may. Private and local addresses
    /// stay refused, here by `check_url` and in the module by its own `*`
    /// rule. The browser itself is off until the user turns it on.
    pub(super) fn allows_any_site(&self) -> bool {
        self.config()
            .http_request
            .allowed_domains
            .iter()
            .any(|domain| domain.trim() == "*")
    }

    pub(super) fn browser_domains(&self) -> Vec<String> {
        self.config()
            .http_request
            .allowed_domains
            .iter()
            .filter(|domain| domain.as_str() != "*")
            .cloned()
            .collect()
    }

    /// Origins a TinyComputer task may open pages on: `*` (any public site)
    /// under "Allow all", else the allowed websites in the module's
    /// `https://.host` spelling. Empty when none are allowed, which a task
    /// refuses, since an empty list means any origin, private ones included,
    /// to the module.
    pub(crate) fn task_origins(&self) -> Vec<String> {
        browser_origins(self)
    }

    pub(super) fn browser_origins(&self) -> Vec<String> {
        browser_origins_for_mode(self, super::policy::browser_allow_all())
    }

    pub(super) pub(super) fn browser_origins_for_mode(&self, allow_all: bool) -> Vec<String> {
        if allow_all {
            // A nonempty, unmatchable origin keeps TinyComputer's document
            // guard active before the caller supplies an explicit URL.
            return vec!["https://.".to_owned()];
        }
        if allows_any_site(self) {
            return vec!["*".to_owned()];
        }
        browser_domains(self)
            .iter()
            .map(|host| {
                if host.contains("://") {
                    host.clone()
                } else {
                    format!("https://.{}", host.trim_start_matches('.'))
                }
            })
            .collect()
    }

}

pub(super) fn validate_session_settings(client: &BrowserClient) -> Result<(), BrowserCallError> {
    let browser = &client.config().browser;
    if browser.profile_mode == "persistent"
        && !browser.profile_path.as_ref().is_some_and(|path| std::path::Path::new(path).is_absolute())
    {
        return Err(BrowserCallError::Policy("persistent profile requires an absolute profile_path".into()));
    }
    if browser.download_dir.as_ref().is_some_and(|path| !std::path::Path::new(path).is_absolute()) {
        return Err(BrowserCallError::Policy("download_dir must be absolute".into()));
    }
    Ok(())
}

