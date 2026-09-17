use super::*;

#[test]
fn percentiles_nearest_rank() {
    let mut v: Vec<u128> = (1..=100).collect();
    v.sort_unstable();
    assert_eq!(percentile(&v, 50), 50);
    assert_eq!(percentile(&v, 95), 95);
    assert_eq!(percentile(&v, 99), 99);
    assert_eq!(percentile(&v, 100), 100);
    assert_eq!(percentile(&[], 50), 0);
    assert_eq!(percentile(&[7], 99), 7);
}

#[test]
fn fleet_agent_is_a_shipped_definition() {
    assert!(
        AgentDefinitionRegistry::builtins_only()
            .get(FLEET_AGENT_ID)
            .is_some(),
        "fleet benchmark must construct a current shipped agent"
    );
}

#[test]
fn latency_summary_none_when_empty() {
    assert!(latency_summary(Vec::new()).is_none());
    let s = latency_summary(vec![10, 20, 30]).unwrap();
    assert_eq!(s.max, 30);
    assert_eq!(s.p50, 20);
}
