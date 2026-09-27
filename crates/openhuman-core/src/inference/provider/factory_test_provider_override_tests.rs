use std::sync::{Arc, Mutex, OnceLock};
use tinyinference_llm::model::ChatModel;

static OVERRIDE: OnceLock<Mutex<Option<Arc<dyn ChatModel<()>>>>> = OnceLock::new();
// The override is process-wide, so tests using it must never replace or clear
// one another's model while an inference call is in flight.
static OVERRIDE_TEST_LOCK: Mutex<()> = Mutex::new(());
fn cell() -> &'static Mutex<Option<Arc<dyn ChatModel<()>>>> {
    OVERRIDE.get_or_init(|| Mutex::new(None))
}

pub(crate) fn current() -> Option<Arc<dyn ChatModel<()>>> {
    cell().lock().unwrap().clone()
}

/// Install a crate-native mock model; the returned guard clears it on drop.
#[must_use]
pub fn install_model(model: Arc<dyn ChatModel<()>>) -> InstallGuard {
    let serial = OVERRIDE_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *cell().lock().unwrap() = Some(model);
    InstallGuard { _serial: serial }
}
pub struct InstallGuard {
    _serial: std::sync::MutexGuard<'static, ()>,
}
impl Drop for InstallGuard {
    fn drop(&mut self) {
        *cell().lock().unwrap() = None;
    }
}
