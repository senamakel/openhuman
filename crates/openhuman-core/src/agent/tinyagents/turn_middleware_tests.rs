use super::*;

#[test]
fn a_poisoned_halt_summary_does_not_abort_stop_reporting() {
    let slot: HaltSummarySlot = Arc::new(std::sync::Mutex::new(None));
    let poisoned = Arc::clone(&slot);
    assert!(std::thread::spawn(move || {
        let _guard = poisoned.lock().unwrap();
        panic!("synthetic prior writer failure");
    })
    .join()
    .is_err());

    record_stop_summary(&slot, 1, "limit reached");

    assert_eq!(
        slot.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_deref(),
        Some("Stopping after 1 model call(s): limit reached")
    );
}
