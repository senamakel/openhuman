use super::*;

#[test]
fn release_tag_uses_the_package_version_without_a_build_sha() {
    if option_env!("OPENHUMAN_BUILD_SHA")
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        assert_eq!(
            build_release_tag(),
            format!("openhuman@{}", env!("CARGO_PKG_VERSION"))
        );
    }
}
