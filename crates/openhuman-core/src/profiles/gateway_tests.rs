use super::*;

const SECRET: &str = "gateway-secret-0123456789abcdef0123456789";

#[test]
fn a_fresh_signature_verifies() {
    let header = sign(SECRET, "alice", 1_000);
    assert!(header.starts_with("t=1000,v1="));
    verify(SECRET, "alice", &header, 1_000).unwrap();
    verify(SECRET, "alice", &header, 1_000 + SIGNATURE_WINDOW_SECS).unwrap();
    verify(SECRET, "alice", &header, 1_000 - SIGNATURE_WINDOW_SECS).unwrap();
}

#[test]
fn a_signature_is_bound_to_the_user_the_secret_and_the_window() {
    let header = sign(SECRET, "alice", 1_000);
    assert!(
        verify(SECRET, "bob", &header, 1_000).is_err(),
        "another user"
    );
    assert!(
        verify("other-secret", "alice", &header, 1_000).is_err(),
        "another secret"
    );
    let stale = verify(SECRET, "alice", &header, 1_000 + SIGNATURE_WINDOW_SECS + 1);
    assert!(stale.unwrap_err().contains("window"));
}

#[test]
fn malformed_signatures_are_refused() {
    for header in ["", "v1=00", "t=1000", "t=abc,v1=00", "t=1000,v1=zz"] {
        assert!(
            verify(SECRET, "alice", header, 1_000).is_err(),
            "{header:?}"
        );
    }
}

#[test]
fn a_tampered_tag_is_refused() {
    let header = sign(SECRET, "alice", 1_000);
    let mut tampered = header.clone();
    let last = tampered.pop().unwrap();
    tampered.push(if last == '0' { '1' } else { '0' });
    assert!(verify(SECRET, "alice", &tampered, 1_000)
        .unwrap_err()
        .contains("does not match"));
}

#[tokio::test]
async fn no_user_header_is_the_operator_plane() {
    assert!(matches!(
        resolve_scope(None, None, SECRET, 0).await,
        Ok(GatewayScope::Operator)
    ));
}

fn id(name: &str) -> ProfileId {
    ProfileId::parse(name).unwrap()
}

#[test]
fn a_profile_held_elsewhere_is_a_409_naming_the_holder() {
    let record = crate::storage::lease::LeaseRecord {
        owner: "node-a".into(),
        endpoint: Some("http://10.0.0.1:7788".into()),
        epoch: 3,
        expires_at_ms: 10_000,
        released: false,
    };
    let refusal = GatewayRefusal::from_open_error(OpenError::HeldElsewhere(record), 4_000);
    assert_eq!(refusal.status, 409);
    assert_eq!(refusal.message, PROFILE_HELD);
    assert_eq!(
        refusal.held_by,
        Some(HeldBy {
            owner: "node-a".into(),
            endpoint: Some("http://10.0.0.1:7788".into()),
            retry_after_ms: 6_000,
        })
    );
}

#[test]
fn a_lease_that_never_expires_reports_a_capped_retry_hint() {
    let record = crate::storage::lease::LeaseRecord {
        owner: "node-a".into(),
        endpoint: None,
        epoch: 1,
        expires_at_ms: u64::MAX,
        released: false,
    };
    let refusal = GatewayRefusal::from_open_error(OpenError::HeldElsewhere(record), 4_000);
    assert_eq!(refusal.held_by.unwrap().retry_after_ms, 60_000);
}

#[test]
fn other_open_errors_keep_their_statuses() {
    let alice = id("alice");
    let status = |error| GatewayRefusal::from_open_error(error, 0).status;
    assert_eq!(status(OpenError::NotProvisioned(alice.clone())), 403);
    assert_eq!(status(OpenError::Full { max: 2 }), 503);
    assert_eq!(status(OpenError::Storage("down".into())), 503);
    let refusal = GatewayRefusal::from_open_error(OpenError::Full { max: 2 }, 0);
    assert!(refusal.held_by.is_none());
}
