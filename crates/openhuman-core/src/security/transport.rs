//! Credential transport checks shared by host-owned network adapters.

/// Whether an HTTP endpoint can safely carry a bearer credential.
///
/// HTTPS endpoints are allowed. Plain HTTP is limited to localhost and IP
/// loopback addresses so local development servers remain usable without
/// sending credentials over a network in cleartext.
pub(crate) fn is_safe_endpoint_for_bearer(endpoint: &str) -> bool {
    let Ok(url) = url::Url::parse(endpoint) else {
        return false;
    };

    match url.scheme() {
        "https" => true,
        "http" => match url.host() {
            Some(url::Host::Domain("localhost")) => true,
            Some(url::Host::Ipv4(address)) => address.is_loopback(),
            Some(url::Host::Ipv6(address)) => address.is_loopback(),
            _ => false,
        },
        _ => false,
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
