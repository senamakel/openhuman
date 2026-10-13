use super::*;
use tinycomputer_bus::site_memory::SavedHint;

fn memory() -> SiteMemory {
    serde_json::from_value(serde_json::json!({
        "hints": [{"hint": {"app": "browser", "key": "k", "role": "button", "name": "Go"}, "ok_at": 7}]
    }))
    .unwrap()
}

fn store() -> (tempfile::TempDir, SiteStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SiteStore::new(sites_dir(dir.path()));
    (dir, store)
}

#[tokio::test]
async fn saved_memory_loads_back_and_empty_save_removes_the_site() {
    let (_dir, store) = store();
    assert!(store.load("example.com").await.unwrap().is_empty());
    assert!(store.save("example.com", &memory()).await.unwrap());
    let back = store.load("example.com").await.unwrap();
    assert_eq!(back, memory());
    let _: &SavedHint = &back.hints[0];
    assert!(store
        .save("example.com", &SiteMemory::default())
        .await
        .unwrap());
    assert!(store.load("example.com").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_site_that_is_not_a_plain_host_is_refused() {
    let (_dir, store) = store();
    for bad in ["", "../x", "a/b", "A.com", ".hidden", "a..b"] {
        assert!(store.save(bad, &memory()).await.is_err(), "{bad}");
        assert!(store.load(bad).await.is_err(), "{bad}");
    }
}

#[tokio::test]
async fn an_unreadable_file_is_set_aside_and_read_as_empty() {
    let (dir, store) = store();
    let sites = sites_dir(dir.path());
    std::fs::create_dir_all(&sites).unwrap();
    std::fs::write(sites.join("x.com.json"), b"not json").unwrap();
    assert!(store.load("x.com").await.unwrap().is_empty());
    assert!(sites.join("x.com.json.corrupt").exists());
    assert_eq!(store.forget(Some("x.com")).await.unwrap(), 1);
}

#[tokio::test]
async fn forgetting_every_site_counts_distinct_sites() {
    let (_dir, store) = store();
    store.save("a.com", &memory()).await.unwrap();
    store.save("b.com", &memory()).await.unwrap();
    assert_eq!(store.forget(None).await.unwrap(), 2);
    assert_eq!(store.forget(None).await.unwrap(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn site_files_are_private_to_their_owner() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, store) = store();
    store.save("a.com", &memory()).await.unwrap();
    let mode = std::fs::metadata(sites_dir(dir.path()).join("a.com.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn site_memory_is_never_published_to_agent_memory_or_prompts() {
    // The adapter must stay a pure file store: no event bus, memory or prompt
    // surface may be named in its source.
    let source = include_str!("site_memory_host.rs");
    let code = source.split("#[cfg(test)]").next().unwrap();
    for forbidden in ["BUS.", "crate::memory", "prompt", "DomainEvent"] {
        let hits = code
            .lines()
            .filter(|l| !l.trim_start().starts_with("//") && l.contains(forbidden))
            .count();
        assert_eq!(hits, 0, "{forbidden}");
    }
}
