//! The wallet's reqwest 0.13 seam with OpenHuman's existing proxy and TLS policy.
//! The rest of the host keeps reqwest 0.12; scope and bypass decisions come from
//! the same ProxyConfig rather than a second policy or environment resolver.

use crate::config::ProxyConfig;

pub(super) fn apply(
    builder: reqwest13::ClientBuilder,
    service: &str,
    config: &ProxyConfig,
) -> reqwest13::ClientBuilder {
    let mut builder = platform_tls(builder);
    if !config.should_apply_to_service(service) {
        return builder;
    }
    let bypass = config.normalized_no_proxy().join(",");
    let no_proxy = reqwest13::NoProxy::from_string(&bypass);
    for (kind, url) in [
        ("all", config.all_proxy.as_deref()),
        ("http", config.http_proxy.as_deref()),
        ("https", config.https_proxy.as_deref()),
    ] {
        let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) else {
            continue;
        };
        let proxy = match kind {
            "all" => reqwest13::Proxy::all(url),
            "http" => reqwest13::Proxy::http(url),
            _ => reqwest13::Proxy::https(url),
        };
        match proxy {
            Ok(proxy) => builder = builder.proxy(proxy.no_proxy(no_proxy.clone())),
            // Proxy URLs can embed credentials, so do not include either the
            // URL or a parser error that could repeat it in diagnostics.
            Err(_) => tracing::warn!(service, kind, "[x402] ignoring invalid proxy URL"),
        }
    }
    builder
}

fn platform_tls(builder: reqwest13::ClientBuilder) -> reqwest13::ClientBuilder {
    #[cfg(target_os = "windows")]
    {
        builder.tls_backend_native()
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Reqwest 0.13 normally uses the platform verifier. Match the host's
        // 0.12 client instead: ring, rustls, and Mozilla's webpki roots.
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
        builder.tls_backend_preconfigured(tls)
    }
}

#[cfg(test)]
#[path = "proxy_compat_tests.rs"]
mod tests;
