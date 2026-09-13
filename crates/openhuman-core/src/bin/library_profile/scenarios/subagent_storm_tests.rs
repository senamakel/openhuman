use super::*;

#[test]
fn percentiles_nearest_rank() {
    let v: Vec<u128> = (1..=100).collect();
    assert_eq!(percentile(&v, 50), 50);
    assert_eq!(percentile(&v, 95), 95);
    assert_eq!(percentile(&v, 99), 99);
    assert_eq!(percentile(&v, 100), 100);
    assert_eq!(percentile(&[], 50), 0);
}

#[test]
fn latency_summary_none_when_empty() {
    assert!(latency_summary(Vec::new()).is_none());
    let s = latency_summary(vec![10, 20, 30]).unwrap();
    assert_eq!(s.max, 30);
    assert_eq!(s.p50, 20);
}

#[test]
fn env_usize_falls_back_on_zero_or_unset() {
    assert_eq!(env_usize("OPENHUMAN_PROFILE_STORM_UNSET_XYZ", 8), 8);
}
