//! TinyComputer RPC handlers registered in the stable `modules` namespace.

use crate::core::runtime::DispatchCtx;
use crate::core::all::ControllerFuture;
use serde_json::{Map, Value};
use tinycomputer_bus::browser::SessionOptions;
use std::sync::Arc;

/// What the settings page's Chrome check shows when Chrome would not start:
/// where to fix a missing Chrome or a Chrome path that names no Chrome, or
/// else the module's own reason.
fn readiness_error(detail: &str) -> String {
    if crate::desktop::browser::chrome_hints::chrome_not_found(detail) {
        "Chrome was not found. Set Chrome path below to the Chrome program (for example \
         /Applications/Google Chrome.app/Contents/MacOS/Google Chrome), save, and test again."
            .to_owned()
    } else if crate::desktop::browser::chrome_hints::chrome_path_unusable(detail) {
        "Chrome could not be started from Chrome path. Set it to the Chrome program itself \
         (for example /Applications/Google Chrome.app/Contents/MacOS/Google Chrome), save, and \
         test again."
            .to_owned()
    } else {
        format!("Chrome could not start: {detail}")
    }
}

pub(crate) fn handle_browser_check_readiness(
    ctx: DispatchCtx,
    _params: Map<String, Value>,
) -> ControllerFuture {
    Box::pin(async move {
        let mut config = ctx.load_config().await?;
        crate::core::runtime::components::require_config(&config, "computer")?;
        // The probe never navigates. Give its disposable session a non-routable
        // origin so an empty website allowlist does not prevent a Chrome check.
        config.http_request.allowed_domains = vec!["example.invalid".into()];
        let client = crate::desktop::browser::BrowserClient::new(Arc::new(config));
        if client.ensure_ready().await.is_err() {
            return Ok(
                serde_json::json!({"module_ready": false, "chrome_ready": false, "error": "TinyComputer module is unavailable; configure a local module override"}),
            );
        }
        let opened = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            client.open_session(SessionOptions::default()),
        )
        .await;
        match opened {
            Ok(Ok(session)) => {
                // A session is always closed after this non-navigating probe.
                let closed = client.close_session(&session.id).await.is_ok();
                Ok(
                    serde_json::json!({"module_ready": true, "chrome_ready": closed,
                    "error": if closed { None } else { Some("Chrome session could not close cleanly") }}),
                )
            }
            Ok(Err(error)) => Ok(
                serde_json::json!({"module_ready": true, "chrome_ready": false,
                "error": readiness_error(&error.to_string())}),
            ),
            Err(_) => Ok(
                serde_json::json!({"module_ready": true, "chrome_ready": false,
                "error": "Chrome launch timed out"}),
            ),
        }
    })
}

pub(crate) fn handle_browser_forget_sites(ctx: DispatchCtx, params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        // A blank or non-text site must not read as "every site".
        let site = match params.get("site") {
            None | Some(Value::Null) => None,
            Some(Value::String(site)) if !site.trim().is_empty() => Some(site.trim().to_owned()),
            Some(_) => {
                return Err(
                    "`site` must name a site (a host or an address); omit it to forget every site"
                        .to_owned(),
                )
            }
        };
        let config = ctx.load_config().await?;
        crate::core::runtime::components::require_config(&config, "computer")?;
        let forgotten = crate::desktop::site_memory::forget(&config, site.as_deref())
            .await
            .map_err(|error| format!("could not forget learned site data: {error}"))?;
        tracing::info!(
            forgotten,
            all = site.is_none(),
            "[browser-sites] forgot learned site data"
        );
        Ok(serde_json::json!({ "forgotten": forgotten }))
    })
}

pub(crate) fn handle_computer_status(ctx: DispatchCtx, params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let load = params.get("load").and_then(Value::as_bool).unwrap_or(false);
        let config = ctx.load_config().await?;
        crate::core::runtime::components::require_config(&config, "computer")?;
        controller_value(super::computer::status(&config, load).await)
    })
}

fn controller_value(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

