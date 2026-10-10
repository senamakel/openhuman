use super::*;
use std::collections::HashSet;

fn item(toolkit: &str, connected: bool) -> crate::agent::prompts::ConnectedIntegration {
    crate::agent::prompts::ConnectedIntegration {
        toolkit: toolkit.into(),
        description: String::new(),
        tools: Vec::new(),
        gated_tools: Vec::new(),
        connected,
        connections: Vec::new(),
        non_active_status: None,
    }
}

#[test]
fn only_connected_toolkits_seed_the_announced_set() {
    let items = vec![
        item("gmail", true),
        item("notion", false),
        item("slack", false),
    ];
    let seeded = connected_toolkit_slugs(&items);
    assert_eq!(seeded, HashSet::from(["gmail".to_string()]));
}

#[test]
fn allowlisted_but_unconnected_toolkits_are_never_announced() {
    let mut announced = HashSet::new();
    let mut pending = Vec::new();
    // The backend lists every allowlisted toolkit; only two are connected.
    let mut current: Vec<_> = (0..130)
        .map(|i| item(&format!("toolkit_{i:03}"), false))
        .collect();
    current.push(item("gmail", true));
    current.push(item("github", true));

    merge_integration_announcements(&mut announced, &mut pending, &current);

    assert_eq!(pending, vec!["github".to_string(), "gmail".to_string()]);
    assert_eq!(
        announced,
        HashSet::from(["gmail".to_string(), "github".to_string()])
    );
}

#[test]
fn a_new_connection_is_announced_once_and_a_revoke_is_dropped() {
    let mut announced = HashSet::from(["gmail".to_string()]);
    let mut pending = Vec::new();

    // Notion becomes connected; gmail is revoked (still listed, not connected).
    let current = vec![
        item("gmail", false),
        item("notion", true),
        item("slack", false),
    ];
    merge_integration_announcements(&mut announced, &mut pending, &current);
    assert_eq!(pending, vec!["notion".to_string()]);
    assert_eq!(announced, HashSet::from(["notion".to_string()]));

    // The same snapshot again queues nothing new.
    merge_integration_announcements(&mut announced, &mut pending, &current);
    assert_eq!(pending, vec!["notion".to_string()]);
}

#[test]
fn a_pending_announcement_for_a_revoked_toolkit_is_withdrawn() {
    let mut announced = HashSet::from(["notion".to_string()]);
    let mut pending = vec!["notion".to_string()];
    merge_integration_announcements(&mut announced, &mut pending, &[item("notion", false)]);
    assert!(pending.is_empty());
    assert!(announced.is_empty());
}
