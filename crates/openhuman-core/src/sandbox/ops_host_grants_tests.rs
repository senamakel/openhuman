use super::*;

#[test]
fn saas_local_grants_never_consult_the_operator_home() {
    let runtime = RuntimeConfig::default();
    let grants = local_jail_grants_with_home(&runtime, true, || {
        panic!("a tenant must not read the operator's home")
    });
    assert!(grants.read_write.is_empty());
    assert!(grants
        .read_only
        .iter()
        .all(|path| path.starts_with("/usr/local") || path.starts_with("/opt")));
}

#[test]
fn single_user_local_grants_keep_the_configured_toolchain_home() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".rustup")).unwrap();
    let runtime = RuntimeConfig::default();
    let grants = local_jail_grants_with_home(&runtime, false, || Some(home.path().to_path_buf()));
    assert!(grants
        .read_only
        .contains(&home.path().join(".rustup").canonicalize().unwrap()));
}
