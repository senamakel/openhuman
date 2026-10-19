//! SaaS core boot requires an operator-selected storage backend.
//!
//! This is a separate integration-test process because building a SaaS core
//! locks the process-wide runtime mode for its lifetime.

use openhuman_core::core::runtime::saas::{SaasConfig, build};

#[path = "support/env_guard.rs"]
mod env_guard;

#[test]
fn a_saas_core_refuses_to_boot_without_a_storage_url() {
    let tmp = tempfile::tempdir().expect("temporary root");
    let root = tmp.path().join("saas");
    std::fs::create_dir(&root).expect("SaaS root");
    set_mode(&root, 0o700);

    let token = root.join("service.token");
    std::fs::write(&token, "saas-storage-test-token-0123456789abcdef\n").unwrap();
    set_mode(&token, 0o600);

    let config = tmp.path().join("operator.toml");
    std::fs::write(
        &config,
        format!("root = {:?}\n", root.display().to_string()),
    )
    .unwrap();

    let _home = env_guard::EnvVarGuard::set_to_path("HOME", tmp.path());
    let _userprofile = env_guard::EnvVarGuard::set_to_path("USERPROFILE", tmp.path());
    let _keyring_backend = env_guard::EnvVarGuard::set("OPENHUMAN_KEYRING_BACKEND", "file");
    let _keyring_key = env_guard::EnvVarGuard::set(
        "OPENHUMAN_KEYRING_MASTER_KEY",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    );
    let mut _unset = Vec::new();
    for name in [
        "OPENHUMAN_WORKSPACE",
        "OPENHUMAN_DEV_CONNECT",
        "OPENHUMAN_BACKEND_SESSION_TOKEN",
        "OPENHUMAN_BACKEND_API_KEY",
        "OPENHUMAN_CORE_TOKEN",
        "OPENHUMAN_APPROVAL_GATE",
        "OPENHUMAN_SANDBOX",
        "OPENHUMAN_STORAGE_URL",
        "OPENHUMAN_NODE_ID",
        "BACKEND_URL",
        "VITE_BACKEND_URL",
    ] {
        _unset.push(env_guard::EnvVarGuard::unset(name));
    }

    let loaded = SaasConfig::load(&config).expect("operator config");
    assert_eq!(loaded.resolved_storage_url(), None);

    let error = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(build(loaded, None, None))
                .map(|_| panic!("SaaS boot without storage must fail"))
                .unwrap_or_else(|error| error.to_string())
        })
        .expect("spawn boot thread")
        .join()
        .expect("boot thread should not panic");

    assert!(
        error.contains("storage URL"),
        "clear storage error expected: {error}"
    );
}

#[cfg(unix)]
fn set_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn set_mode(_: &std::path::Path, _: u32) {}
