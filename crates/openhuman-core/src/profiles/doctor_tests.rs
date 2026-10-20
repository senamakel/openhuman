//! SaaS doctor selection fixtures. This module is included by
//! `profiles::doctor` when the tenant resolver lands.

use super::{resolve_with_loader, DoctorCaller};
use crate::profiles::ProfileId;
use std::future::Ready;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadKind {
    Registry,
    Config,
    Credential,
    Lease,
}

#[derive(Default)]
struct ReadSpy(Mutex<Vec<(ReadKind, ProfileId)>>);

impl ReadSpy {
    fn record_selected_profile(&self, id: &ProfileId) {
        let mut reads = self.0.lock().unwrap();
        for kind in [
            ReadKind::Registry,
            ReadKind::Config,
            ReadKind::Credential,
            ReadKind::Lease,
        ] {
            reads.push((kind, id.clone()));
        }
    }

    fn snapshot(&self) -> Vec<(ReadKind, ProfileId)> {
        self.0.lock().unwrap().clone()
    }
}

fn spy_loader(
    spy: Arc<ReadSpy>,
) -> impl FnOnce(ProfileId) -> Ready<Result<ProfileId, String>> {
    move |id| {
        spy.record_selected_profile(&id);
        std::future::ready(Ok(id))
    }
}

fn profile(raw: &str) -> ProfileId {
    ProfileId::parse(raw).unwrap()
}

#[tokio::test]
async fn a_tenant_foreign_profile_is_rejected_before_any_tenant_read() {
    let alice = profile("alice");
    let bob = profile("bob");
    let reads = Arc::new(ReadSpy::default());

    let result = resolve_with_loader(
        Some(DoctorCaller::Tenant(alice)),
        Some(bob.as_str()),
        spy_loader(Arc::clone(&reads)),
    )
    .await;

    assert!(result.is_err(), "a tenant cannot inspect another profile");
    assert!(
        reads.snapshot().is_empty(),
        "authorization must precede registry, config, credential, and lease reads"
    );
}

#[tokio::test]
async fn an_unscoped_saas_call_cannot_become_an_operator_profile_read() {
    let bob = profile("bob");
    let reads = Arc::new(ReadSpy::default());

    let result = resolve_with_loader(
        None,
        Some(bob.as_str()),
        spy_loader(Arc::clone(&reads)),
    )
    .await;

    assert!(result.is_err(), "missing task scope must fail closed");
    assert!(reads.snapshot().is_empty());
}

#[tokio::test]
async fn a_tenant_without_a_requested_id_loads_only_its_own_profile() {
    let alice = profile("alice");
    let bob = profile("bob");
    let reads = Arc::new(ReadSpy::default());

    let result = resolve_with_loader(
        Some(DoctorCaller::Tenant(alice.clone())),
        None,
        spy_loader(Arc::clone(&reads)),
    )
    .await
    .unwrap();

    assert_eq!(result, Some(alice.clone()));
    assert_eq!(
        reads.snapshot(),
        [
            (ReadKind::Registry, alice.clone()),
            (ReadKind::Config, alice.clone()),
            (ReadKind::Credential, alice.clone()),
            (ReadKind::Lease, alice),
        ]
    );
    assert!(!reads.snapshot().iter().any(|(_, id)| id == &bob));
}

#[tokio::test]
async fn an_operator_can_select_any_profile_after_scope_is_verified() {
    let bob = profile("bob");
    let reads = Arc::new(ReadSpy::default());

    let result = resolve_with_loader(
        Some(DoctorCaller::Operator),
        Some(bob.as_str()),
        spy_loader(Arc::clone(&reads)),
    )
    .await
    .unwrap();

    assert_eq!(result, Some(bob.clone()));
    assert_eq!(
        reads.snapshot(),
        [
            (ReadKind::Registry, bob.clone()),
            (ReadKind::Config, bob.clone()),
            (ReadKind::Credential, bob.clone()),
            (ReadKind::Lease, bob),
        ]
    );
}

#[tokio::test]
async fn an_operator_without_a_selected_profile_does_not_load_a_tenant() {
    let reads = Arc::new(ReadSpy::default());

    let result = resolve_with_loader(
        Some(DoctorCaller::Operator),
        None,
        spy_loader(Arc::clone(&reads)),
    )
    .await
    .unwrap();

    assert!(result.is_none(), "the operator keeps its own normal config path");
    assert!(reads.snapshot().is_empty());
}

#[tokio::test]
async fn malformed_profile_ids_are_rejected_before_tenant_reads() {
    let reads = Arc::new(ReadSpy::default());

    let result = resolve_with_loader(
        Some(DoctorCaller::Operator),
        Some("../bob"),
        spy_loader(Arc::clone(&reads)),
    )
    .await;

    assert!(result.is_err());
    assert!(reads.snapshot().is_empty());
}

