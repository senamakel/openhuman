use super::*;
use crate::config::SandboxBackend;

#[test]
fn every_backend_name_parses_back_to_itself() {
    for backend in [
        SandboxBackend::Auto,
        SandboxBackend::Landlock,
        SandboxBackend::Firejail,
        SandboxBackend::Bubblewrap,
        SandboxBackend::Docker,
        SandboxBackend::OpenShell,
        SandboxBackend::None,
    ] {
        let name = backend_name(&backend);
        assert_eq!(
            serde_json::to_value(&backend).unwrap(),
            serde_json::json!(name)
        );
        assert_eq!(backend_name(&parse_backend(name).unwrap()), name);
    }
}

#[test]
fn an_unknown_backend_names_the_valid_ones() {
    let refused = parse_backend("vm").unwrap_err();
    assert!(refused.contains("unknown sandbox backend 'vm'"));
    assert!(refused.contains("openshell"));
}
