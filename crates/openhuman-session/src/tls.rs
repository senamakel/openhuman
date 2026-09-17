//! Platform-conditional TLS backend selection, mirroring the core's
//! `util::tls::tls_client_builder`: Windows uses schannel (`native-tls`) so the
//! OS certificate store — including corporate CAs installed by TLS-inspecting
//! proxies — is honoured; everywhere else uses `rustls` + webpki roots.

pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    let b = reqwest::Client::builder();
    #[cfg(target_os = "windows")]
    let b = b.use_native_tls();
    #[cfg(not(target_os = "windows"))]
    let b = b.use_rustls_tls();
    b
}
