use super::*;

fn store(tmp: &tempfile::TempDir) -> HttpCredentialsStore {
    HttpCredentialsStore::new(tmp.path(), false)
}

#[test]
fn credential_resolution_ignores_an_absent_parsed_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = store(&tmp);
    assert!(resolve_http_credential(&store, None).unwrap().is_none());
}

#[test]
fn credential_resolution_fails_closed_for_empty_or_unknown_names() {
    let tmp = tempfile::TempDir::new().unwrap();
    let store = store(&tmp);
    assert!(resolve_http_credential(&store, Some(" ")).is_err());
    assert!(resolve_http_credential(&store, Some("missing")).is_err());
}
