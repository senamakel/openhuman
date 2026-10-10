use super::*;
use crate::storage::{MemoryStorage, Scope, StorageBackend};
use chrono::{Duration, Utc};

use super::super::types::TokenUsage;

fn docs_in(storage: &MemoryStorage, scope: &str) -> CostDocs {
    CostDocs::over(&storage.for_scope(&Scope::new(scope).unwrap()).unwrap())
}

fn record(model: &str, cost: f64, secs_ago: i64) -> CostRecord {
    let mut usage = TokenUsage::new(model, 10, 5, 1.0, 2.0);
    usage.cost_usd = cost;
    usage.timestamp = Utc::now() - Duration::seconds(secs_ago);
    CostRecord::new("session", usage)
}

#[test]
fn records_round_trip_oldest_first() {
    let docs = docs_in(&MemoryStorage::new(), "local");
    let newer = record("b/model", 0.5, 1);
    let older = record("a/model", 0.25, 60);
    docs.add(&newer).unwrap();
    docs.add(&older).unwrap();
    let all = docs.all().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, older.id);
    assert_eq!(all[1].id, newer.id);
    assert_eq!(all[1].usage.cost_usd, 0.5);
    assert_eq!(all[1].session_id, "session");
}

#[test]
fn a_record_is_written_once() {
    let docs = docs_in(&MemoryStorage::new(), "local");
    let rec = record("a/model", 0.1, 0);
    docs.add(&rec).unwrap();
    assert!(docs.add(&rec).is_err());
    assert_eq!(docs.all().unwrap().len(), 1);
}

#[test]
fn scopes_do_not_see_each_others_costs() {
    let storage = MemoryStorage::new();
    let alice = docs_in(&storage, "alice");
    let bob = docs_in(&storage, "bob");
    alice.add(&record("a/model", 1.0, 0)).unwrap();
    assert_eq!(alice.all().unwrap().len(), 1);
    assert!(bob.all().unwrap().is_empty());
}
