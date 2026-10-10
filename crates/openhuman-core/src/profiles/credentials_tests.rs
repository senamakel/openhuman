use super::*;
use crate::profiles::layout::{profile_config, ProfileLayout};
use crate::profiles::ProfileId;
use crate::security::credentials::session_support::{
    resolve_backend_credential, BackendCredential,
};

/// Credential secrets live in the process keyring keyed by agent id, which
/// every test in this binary shares; each test therefore uses its own users.
fn profile(tmp: &tempfile::TempDir, user: &str) -> Config {
    let id = ProfileId::for_user(
        &format!("{user}-{}", uuid::Uuid::new_v4()),
        crate::profiles::ProfileIdMode::Raw,
    )
    .unwrap();
    let layout = ProfileLayout::new(tmp.path(), &id);
    std::fs::create_dir_all(&layout.workspace_dir).unwrap();
    profile_config(&layout, &id)
}

#[test]
fn each_profile_resolves_only_its_own_credential() {
    let tmp = tempfile::tempdir().unwrap();
    let alice = profile(&tmp, "alice");
    let bob = profile(&tmp, "bob");
    store(&alice, UserCredentialKind::Session, "alice-jwt", None).unwrap();
    store(&bob, UserCredentialKind::ApiKey, "bob-key", None).unwrap();

    assert_eq!(
        resolve_backend_credential(&alice).unwrap(),
        BackendCredential::Session("alice-jwt".into())
    );
    assert_eq!(
        resolve_backend_credential(&bob).unwrap(),
        BackendCredential::ApiKey("bob-key".into())
    );
}

#[test]
fn clearing_one_profile_leaves_the_other() {
    let tmp = tempfile::tempdir().unwrap();
    let alice = profile(&tmp, "alice");
    let bob = profile(&tmp, "bob");
    store(&alice, UserCredentialKind::Session, "alice-jwt", None).unwrap();
    store(&bob, UserCredentialKind::Session, "bob-jwt", None).unwrap();
    assert!(clear(&alice).unwrap());
    assert!(!has(&alice));
    assert!(has(&bob));
    assert!(!clear(&alice).unwrap(), "nothing left to clear");
}

#[test]
fn an_expired_session_is_rejected_locally() {
    let tmp = tempfile::tempdir().unwrap();
    let alice = profile(&tmp, "alice");
    store(
        &alice,
        UserCredentialKind::Session,
        "alice-jwt",
        Some("2000-01-01T00:00:00Z"),
    )
    .unwrap();
    let err = resolve_backend_credential(&alice).unwrap_err();
    assert!(err.contains("SESSION_EXPIRED"), "{err}");
}

#[test]
fn bad_input_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let alice = profile(&tmp, "alice");
    assert!(store(&alice, UserCredentialKind::Session, "  ", None).is_err());
    assert!(store(&alice, UserCredentialKind::Session, "t", Some("tomorrow")).is_err());
    assert!(!has(&alice));
}

#[test]
fn installing_one_kind_replaces_the_other() {
    let tmp = tempfile::tempdir().unwrap();
    let alice = profile(&tmp, "alice-rotates");
    store(&alice, UserCredentialKind::ApiKey, "old-key", None).unwrap();
    store(&alice, UserCredentialKind::Session, "new-jwt", None).unwrap();
    assert_eq!(
        resolve_backend_credential(&alice).unwrap(),
        BackendCredential::Session("new-jwt".into()),
        "the old API key no longer wins"
    );
    store(&alice, UserCredentialKind::ApiKey, "newer-key", None).unwrap();
    assert_eq!(
        resolve_backend_credential(&alice).unwrap(),
        BackendCredential::ApiKey("newer-key".into())
    );
    assert!(clear(&alice).unwrap());
}

#[test]
fn every_profile_of_the_other_kind_is_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let alice = profile(&tmp, "alice-profiles");
    // A non-default, active API-key profile.
    AuthService::from_config(&alice)
        .store_provider_token(
            api_key::API_KEY_PROVIDER,
            "other",
            "side-key",
            HashMap::new(),
            true,
        )
        .unwrap();
    store(&alice, UserCredentialKind::Session, "jwt", None).unwrap();
    assert_eq!(
        resolve_backend_credential(&alice).unwrap(),
        BackendCredential::Session("jwt".into())
    );
    assert!(clear(&alice).unwrap());
    assert!(!has(&alice));
}

#[test]
fn concurrent_stores_of_different_kinds_leave_exactly_one_kind() {
    let tmp = tempfile::tempdir().unwrap();
    for round in 0..8 {
        let user = profile(&tmp, &format!("race{round}"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let spawn = |kind, token: &'static str| {
            let (user, barrier) = (user.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store(&user, kind, token, None).unwrap();
            })
        };
        let session = spawn(UserCredentialKind::Session, "race-jwt");
        let key = spawn(UserCredentialKind::ApiKey, "race-key");
        session.join().unwrap();
        key.join().unwrap();

        // Whichever store ran last owns the profile; the other kind is gone.
        let provider_present = |provider: &str| {
            let auth = AuthService::from_config(&user);
            let id = crate::security::credentials::normalize_provider(provider).unwrap();
            auth.load_profiles()
                .unwrap()
                .profiles
                .values()
                .any(|profile| profile.provider == id)
        };
        let has_key = provider_present(api_key::API_KEY_PROVIDER);
        let has_session = provider_present(APP_SESSION_PROVIDER);
        assert!(
            has_key != has_session,
            "round {round}: exactly one kind must remain (key={has_key}, session={has_session})"
        );
    }
}
