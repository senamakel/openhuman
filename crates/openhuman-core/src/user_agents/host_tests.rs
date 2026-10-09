use super::*;

fn host(tmp: &tempfile::TempDir, max_open: usize, idle_secs: u64) -> AgentHost {
    let mut saas = SaasConfig::new(tmp.path());
    saas.max_agents_open = max_open;
    saas.idle_evict_secs = idle_secs;
    AgentHost::new(saas, CoreContext::for_test(DomainSet::full(), None))
}

fn agent(name: &str) -> UserAgentId {
    UserAgentId::for_user(name).unwrap()
}

#[test]
fn provisioning_creates_the_layout_once() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 60);
    let id = agent("alice");
    assert!(host.provision(&id).unwrap());
    assert!(!host.provision(&id).unwrap(), "second provision is a no-op");
    let layout = UserAgentLayout::new(tmp.path(), &id);
    assert!(layout.workspace_dir.is_dir() && layout.sandbox_dir.is_dir());
    let summary = host.summary(&id).unwrap().unwrap();
    assert_eq!(summary.agent_id, id);
    assert!(!summary.open);
}

#[test]
fn only_provisioned_agents_open() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 60);
    let err = host.open(&agent("nobody")).unwrap_err();
    assert!(err.contains("not provisioned"), "{err}");
}

#[test]
fn an_open_agent_runs_under_its_own_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 60);
    let (a, b) = (agent("alice"), agent("bob"));
    host.provision(&a).unwrap();
    host.provision(&b).unwrap();
    let state_a = host.open(&a).unwrap();
    let state_b = host.open(&b).unwrap();

    assert_eq!(state_a.context().session_agent(), Some(a.as_str()));
    assert_eq!(state_b.context().session_agent(), Some(b.as_str()));
    let config_a = state_a.context().embedder_config().unwrap();
    let config_b = state_b.context().embedder_config().unwrap();
    assert_ne!(config_a.workspace_dir, config_b.workspace_dir);
    assert!(config_a.workspace_dir.starts_with(tmp.path()));
    assert_eq!(state_a.context().domains(), user_domains());
    assert!(!state_a.context().user_skill_roots());

    // Re-opening hands back the same state.
    assert!(Arc::ptr_eq(&state_a, &host.open(&a).unwrap()));
    assert_eq!(host.open_count(), 2);
}

#[test]
fn a_full_host_evicts_the_least_recently_used_idle_agent() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 2, 3600);
    let ids: Vec<_> = ["a", "b", "c"].iter().map(|n| agent(n)).collect();
    for id in &ids {
        host.provision(id).unwrap();
    }
    drop(host.open(&ids[0]).unwrap());
    drop(host.open(&ids[1]).unwrap());
    drop(host.open(&ids[2]).unwrap());
    assert_eq!(host.open_count(), 2);
    assert!(!host.is_open(&ids[0]), "the oldest idle agent made room");
    assert!(host.is_open(&ids[1]) && host.is_open(&ids[2]));
}

#[test]
fn an_agent_in_use_is_never_evicted() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 1, 0);
    let (a, b) = (agent("a"), agent("b"));
    host.provision(&a).unwrap();
    host.provision(&b).unwrap();
    let held = host.open(&a).unwrap();
    host.evict_idle();
    assert!(host.is_open(&a), "held agents survive an idle sweep");
    let err = host.open(&b).unwrap_err();
    assert!(err.contains("slots are in use"), "{err}");
    drop(held);
    host.open(&b).unwrap();
}

#[test]
fn idle_agents_are_swept() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 0);
    let id = agent("a");
    host.provision(&id).unwrap();
    drop(host.open(&id).unwrap());
    host.evict_idle();
    assert_eq!(host.open_count(), 0);
}

#[test]
fn deprovisioning_archives_and_closes() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 60);
    let id = agent("a");
    host.provision(&id).unwrap();
    drop(host.open(&id).unwrap());
    assert!(host.deprovision(&id).unwrap());
    assert!(!host.is_open(&id));
    assert!(host.summary(&id).unwrap().is_none());
    let archived: Vec<_> = std::fs::read_dir(layout::archive_dir(tmp.path()))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(archived.len(), 1, "state is archived, not deleted");
    assert!(!host.deprovision(&id).unwrap());
}

#[test]
fn list_reports_provisioned_agents_only() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 60);
    assert!(host.list().unwrap().is_empty());
    let (a, b) = (agent("a"), agent("b"));
    host.provision(&a).unwrap();
    host.provision(&b).unwrap();
    std::fs::create_dir_all(layout::agents_dir(tmp.path()).join("not-an-agent")).unwrap();
    let _held = host.open(&b).unwrap();
    let listed = host.list().unwrap();
    assert_eq!(listed.len(), 2);
    let open: Vec<_> = listed
        .iter()
        .filter(|s| s.open)
        .map(|s| &s.agent_id)
        .collect();
    assert_eq!(open, vec![&b]);
}

#[test]
fn a_reprovisioned_agent_does_not_inherit_the_old_credential() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 60);
    let id = agent(&format!("returning-{}", uuid::Uuid::new_v4()));
    host.provision(&id).unwrap();
    let config = host.open(&id).unwrap().config.clone();
    super::super::credentials::store(
        &config,
        super::super::credentials::UserCredentialKind::Session,
        "old-session",
        None,
    )
    .unwrap();
    assert!(host.summary(&id).unwrap().unwrap().has_credential);
    host.deprovision(&id).unwrap();
    host.provision(&id).unwrap();
    assert!(
        !host.summary(&id).unwrap().unwrap().has_credential,
        "deprovisioning must forget the credential"
    );
}

#[test]
fn the_first_open_settles_a_previous_process_once() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 0);
    let id = agent(&format!("recover-{}", uuid::Uuid::new_v4()));
    host.provision(&id).unwrap();
    drop(host.open(&id).unwrap());
    assert!(host.recovered.lock().unwrap().contains(&id));
    host.evict_idle();
    drop(host.open(&id).unwrap());
    assert_eq!(
        host.recovered.lock().unwrap().len(),
        1,
        "a re-open after eviction does not sweep again"
    );
}

#[test]
fn recovery_of_an_empty_workspace_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let id = agent("empty");
    recover_workspace(&id, tmp.path());
}

#[test]
fn an_agent_in_use_is_not_archived_from_under_it() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 60);
    let id = agent("alice");
    host.provision(&id).unwrap();
    let state = host.open(&id).unwrap();
    let err = host.deprovision(&id).unwrap_err();
    assert!(err.contains("in use"), "{err}");
    assert!(host.is_open(&id), "a refused deprovision leaves it open");
    drop(state);
    assert!(host.deprovision(&id).unwrap());
    assert!(!host.is_open(&id));
}

#[test]
fn deprovisioning_twice_in_a_second_archives_twice() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 60);
    let id = agent("alice");
    for _ in 0..2 {
        host.provision(&id).unwrap();
        assert!(host.deprovision(&id).unwrap());
    }
    let archived = std::fs::read_dir(layout::archive_dir(tmp.path()))
        .unwrap()
        .count();
    assert_eq!(archived, 2);
}

#[test]
fn opening_an_open_agent_sweeps_the_idle_ones() {
    let tmp = tempfile::tempdir().unwrap();
    // Everything is idle the moment it is released.
    let host = host(&tmp, 4, 0);
    let (a, b) = (agent("alice"), agent("bob"));
    host.provision(&a).unwrap();
    host.provision(&b).unwrap();
    drop(host.open(&a).unwrap());
    let held = host.open(&b).unwrap();
    // Re-opening bob (already open) closes idle alice.
    let again = host.open(&b).unwrap();
    assert!(!host.is_open(&a), "alice was idle and is closed");
    assert!(host.is_open(&b), "bob is in use and stays");
    drop((held, again));
}

#[test]
fn one_unreadable_agent_does_not_hide_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 4, 60);
    let (a, b) = (agent("alice"), agent("bob"));
    host.provision(&a).unwrap();
    host.provision(&b).unwrap();
    std::fs::write(
        UserAgentLayout::new(tmp.path(), &a).meta_path,
        "not toml = [",
    )
    .unwrap();
    let listed = host.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].agent_id, b);
}

#[test]
fn a_provisioned_config_needs_no_agent_slot() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 1, 60);
    let (a, b) = (agent("alice"), agent("bob"));
    host.provision(&a).unwrap();
    host.provision(&b).unwrap();
    let _held = host.open(&a).unwrap();
    assert!(host.open(&b).is_err(), "the only slot is taken");
    assert!(host.provisioned_config(&b).is_ok());
    assert!(host.provisioned_config(&agent("nobody")).is_err());
}
