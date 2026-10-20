//! Canonical segments for dynamically configured doctor check identifiers.

/// Encodes an arbitrary configured name as a valid check-id segment.
///
/// Native lowercase segments containing only lowercase letters, digits,
/// underscores, and dashes keep their configured spelling, except names that
/// start with the reserved `__x` escape namespace. Invalid names are encoded
/// as `__x_i_<hex UTF-8 bytes>`; valid names inside the reserved namespace are
/// encoded as `__x_v_<hex UTF-8 bytes>`. The separate markers make the mapping
/// collision-free, including for literal names that look like encoded ids.
pub(super) fn canonical_segment(value: &str) -> String {
    let native = !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        });
    if native && !value.starts_with("__x") {
        return value.to_owned();
    }

    let kind = if native { 'v' } else { 'i' };
    let mut encoded = format!("__x_{kind}_");
    for byte in value.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::canonical_segment;

    #[test]
    fn valid_underscores_are_preserved() {
        assert_eq!(canonical_segment("server_name"), "server_name");
        assert_eq!(canonical_segment("server-name_2"), "server-name_2");
    }

    #[test]
    fn invalid_names_use_the_reserved_escape_namespace() {
        assert_eq!(
            canonical_segment("Server A/α"),
            "__x_i_53657276657220412fceb1"
        );
        assert_eq!(canonical_segment(""), "__x_i_");
        assert_eq!(canonical_segment("__x"), "__x_v_5f5f78");
    }

    #[test]
    fn literal_escape_looking_names_do_not_collide_with_encoded_names() {
        let encoded = canonical_segment("Server");
        let literal = canonical_segment("__x_i_536572766572");

        assert_eq!(encoded, "__x_i_536572766572");
        assert!(literal.starts_with("__x_v_"));
        assert_ne!(encoded, literal);
    }

    #[test]
    fn distinct_configured_names_remain_distinct_after_encoding() {
        use std::collections::HashSet;

        let names = [
            "server_name",
            "Server_name",
            "server name",
            "server/name",
            "",
            "__x_i_736572766572",
        ];
        let ids = names
            .iter()
            .map(|name| canonical_segment(name))
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), names.len());
        assert!(ids.iter().all(|id| {
            id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            })
        }));
    }
}
