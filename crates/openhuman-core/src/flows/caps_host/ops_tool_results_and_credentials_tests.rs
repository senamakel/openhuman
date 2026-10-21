use super::*;

/// The host applies its historic credential-id trimming after the module
/// returns the parser's exact name.
#[test]
fn http_credential_lookup_trims_the_module_parsed_name() {
    let (_dir, store) = http_cred_store();
    store
        .upsert(&HttpCredential::bearer("payments", "sk_live_secret"))
        .unwrap();

    let credential = resolve_http_credential(&store, Some(" payments "))
        .expect("lookup succeeds")
        .expect("the normalized credential exists");
    assert_eq!(credential.name, "payments");
    assert_eq!(credential.to_header().unwrap().1, "Bearer sk_live_secret");
}

#[test]
fn parsed_credential_lookup_distinguishes_absent_empty_and_padded_names() {
    let (_dir, store) = http_cred_store();
    store
        .upsert(&HttpCredential::bearer("payments", "sk_live_secret"))
        .unwrap();

    assert!(resolve_http_credential(&store, None).unwrap().is_none());
    assert!(resolve_http_credential(&store, Some("   ")).is_err());
    let credential = resolve_http_credential(&store, Some(" payments "))
        .expect("padded host lookup succeeds")
        .expect("the host trims a padded ID to the existing credential");
    assert_eq!(credential.name, "payments");
}

#[test]
fn unknown_parsed_credential_name_fails_closed() {
    let (_dir, store) = http_cred_store();
    assert!(resolve_http_credential(&store, Some("ghost")).is_err());
}

#[test]
fn absent_and_empty_parsed_names_do_not_select_a_credential() {
    let (_dir, store) = http_cred_store();
    assert!(resolve_http_credential(&store, None).unwrap().is_none());
    assert!(resolve_http_credential(&store, Some("   ")).is_err());
}
