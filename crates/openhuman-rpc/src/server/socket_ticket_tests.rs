use super::*;

#[test]
fn tickets_bind_audience_profile_and_expiry() {
    let ticket = mint("profile-alice", 100, &"s".repeat(32)).unwrap();
    let claims = verify(&ticket, 130, &"s".repeat(32)).unwrap();
    assert_eq!(claims.profile, "profile-alice");
    assert_eq!(claims.aud, AUDIENCE);
    assert!(verify(&ticket, 131, &"s".repeat(32)).is_err());
    assert!(verify(&ticket, 100, &"x".repeat(32)).is_err());
    let mut tampered = ticket;
    tampered.push('x');
    assert!(verify(&tampered, 100, &"s".repeat(32)).is_err());
}

#[test]
fn ticket_secret_must_be_long_enough() {
    assert!(mint("profile", 1, "short").is_err());
}
