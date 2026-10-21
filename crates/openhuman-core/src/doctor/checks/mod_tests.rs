use super::*;
use crate::doctor::contract::DoctorTier;

#[test]
fn memory_storage_and_keyring_checks_are_registered_at_their_declared_tiers() {
    let checks = builtins();
    let tier = |id: &str| {
        checks
            .iter()
            .find(|check| check.id() == id)
            .map(|check| check.tier())
    };

    assert_eq!(tier("memory/binding"), Some(DoctorTier::Static));
    assert_eq!(tier("memory/health"), Some(DoctorTier::Connect));
    assert_eq!(tier("storage/connect"), Some(DoctorTier::Connect));
    assert_eq!(tier("storage/layout"), Some(DoctorTier::Static));
    assert_eq!(tier("secrets/keyring"), Some(DoctorTier::Connect));
}
