use super::*;

#[test]
fn build_roster_constructs_bare_agents_with_isolated_workspaces() {
    let roster = build_roster(8).expect("8-agent roster builds");
    assert_eq!(roster.agents.len(), 8);
    assert_eq!(roster._workspaces.len(), 8);
    // Each agent got a distinct workspace directory.
    let mut dirs: Vec<_> = roster
        ._workspaces
        .iter()
        .map(|w| w.path().to_path_buf())
        .collect();
    dirs.sort();
    dirs.dedup();
    assert_eq!(dirs.len(), 8, "workspaces must be isolated per agent");
}

#[test]
fn warm_up_turn_completes_without_network() {
    // The agent-turn future is large in debug builds. Run it on a worker
    // with explicit stack headroom instead of libtest's smaller default.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .thread_stack_size(8 * 1024 * 1024)
        .build()
        .expect("test runtime builds");
    runtime
        .block_on(runtime.spawn(async {
            let mut roster = build_roster(1).expect("1-agent roster builds");
            warm_up(&mut roster).await.expect("warm-up turn completes");
            // The mock provider reports usage, so last_turn_usage is populated —
            // proving the embedding cost-metering contract works on the bare Agent.
            assert!(
                roster.agents[0].last_turn_usage().is_some(),
                "usage should be readable after a turn"
            );
        }))
        .expect("warm-up task joins");
}
