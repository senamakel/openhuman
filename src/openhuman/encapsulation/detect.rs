//! Platform auto-detection. Picks the strongest available backend.

use std::sync::Arc;

use super::jail::JailBackend;
use super::noop::NoopBackend;

pub fn pick_backend() -> Arc<dyn JailBackend> {
    #[cfg(target_os = "linux")]
    {
        let lb = super::linux::LandlockBackend::new();
        if lb.is_available() {
            log::info!("[encapsulation] backend=landlock");
            return Arc::new(lb);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let sb = super::macos::SeatbeltBackend::new();
        if sb.is_available() {
            log::info!("[encapsulation] backend=seatbelt");
            return Arc::new(sb);
        }
    }
    #[cfg(target_os = "windows")]
    {
        let ac = super::windows::AppContainerBackend::new();
        if ac.is_available() {
            log::info!("[encapsulation] backend=appcontainer");
            return Arc::new(ac);
        }
    }
    log::warn!("[encapsulation] no OS sandbox available, falling back to noop");
    Arc::new(NoopBackend)
}
