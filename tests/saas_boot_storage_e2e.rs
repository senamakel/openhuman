//! SaaS core boot requires an operator-selected storage backend.
//!
//! This is a separate integration-test process because building a SaaS core
//! locks the process-wide runtime mode for its lifetime.

use openhuman_core::core::runtime::saas::{build, SaasConfig};

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
        "OPENHUMAN_KEYRING_MASTER_KEY_FILE",
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

    let (server_error, boot_error, local_fallback_error) = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let server_error = runtime
                .block_on(openhuman_rpc::server::run_server_saas(None, None, &config))
                .expect_err("SaaS server must refuse boot without storage")
                .to_string();
            let boot_error = match runtime.block_on(build(loaded, None, None)) {
                Err(error) => error.to_string(),
                Ok(_) => panic!("SaaS boot without storage must fail"),
            };

            std::fs::write(
                &config,
                format!(
                    "root = {:?}\nstorage_url = \"memory\"\n",
                    root.display().to_string()
                ),
            )
            .unwrap();
            let configured = SaasConfig::load(&config).expect("configured operator config");
            openhuman_core::storage::install(std::sync::Arc::new(
                openhuman_core::storage::MemoryStorage::new(),
            ));
            let core = runtime
                .block_on(build(configured, None, None))
                .expect("a configured SaaS storage backend permits boot");
            openhuman_core::storage::clear();
            let fallback_error = match openhuman_core::storage::local::backend_for(
                &root.join("profiles/alice/workspace"),
            ) {
                Err(error) => error.to_string(),
                Ok(_) => panic!("SaaS must refuse workspace-local storage fallback"),
            };
            drop(core);
            (server_error, boot_error, fallback_error)
        })
        .expect("spawn boot thread")
        .join()
        .expect("boot thread should not panic");

    assert!(
        server_error.contains("storage URL"),
        "clear RPC server storage error expected: {server_error}"
    );
    assert!(
        boot_error.contains("storage URL"),
        "clear storage error expected: {boot_error}"
    );
    assert!(
        local_fallback_error.contains("SaaS"),
        "clear local-fallback error expected: {local_fallback_error}"
    );
}

#[cfg(unix)]
fn set_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn set_mode(_: &std::path::Path, _: u32) {}
