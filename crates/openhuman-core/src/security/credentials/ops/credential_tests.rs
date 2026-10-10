use super::*;

#[test]
fn process_wide_credentials_are_single_user_only() {
    assert!(process_credential_refusal(false, "set_credential").is_ok());
    let err = process_credential_refusal(true, "set_credential").unwrap_err();
    assert!(err.contains("profiles.set_credential"), "{err}");
}
