//! Screen-lock privacy hook: pauses always-on capture while the screen is
//! locked so nothing spoken at the lock screen is ever transcribed.

use super::LOG_PREFIX;
#[cfg(target_os = "macos")]
use super::PAUSED;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;

/// Poll the screen-lock state and drive [`PAUSED`] so always-on never captures
/// what is spoken at the lock screen. macOS-only for now (uses the Quartz
/// session dictionary); other platforms never pause (no lock signal yet).
pub(super) fn spawn_lock_watcher() {
    #[cfg(target_os = "macos")]
    tokio::spawn(async move {
        let mut last = false;
        loop {
            let locked = macos_lock::is_screen_locked();
            if locked != last {
                log::info!(
                    "{LOG_PREFIX} screen {} → {}",
                    if locked { "locked" } else { "unlocked" },
                    if locked { "pausing" } else { "resuming" }
                );
                PAUSED.store(locked, Ordering::Relaxed);
                last = locked;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
    #[cfg(not(target_os = "macos"))]
    {
        log::info!("{LOG_PREFIX} screen-lock watcher unavailable on this platform");
    }
}

/// macOS screen-lock detection via the Quartz session dictionary.
///
/// `CGSessionCopyCurrentDictionary` exposes `CGSSessionScreenIsLocked`; we read
/// it defensively (null dict ⇒ no session, treated as locked; missing/odd value
/// ⇒ unlocked) and never assume the CF value's concrete type without checking.
#[cfg(target_os = "macos")]
mod macos_lock {
    use std::ffi::{c_void, CString};

    type CFTypeRef = *const c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSessionCopyCurrentDictionary() -> CFTypeRef;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        fn CFStringCreateWithCString(alloc: CFTypeRef, c: *const i8, enc: u32) -> CFTypeRef;
        fn CFGetTypeID(v: CFTypeRef) -> usize;
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(b: CFTypeRef) -> u8;
        fn CFNumberGetTypeID() -> usize;
        fn CFNumberGetValue(n: CFTypeRef, the_type: i64, out: *mut c_void) -> u8;
        fn CFRelease(v: CFTypeRef);
    }
    const KCF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const KCF_NUMBER_SINT32: i64 = 3;

    /// True when the screen is locked (or there is no active GUI session).
    pub fn is_screen_locked() -> bool {
        // SAFETY: standard Quartz/CoreFoundation calls. Ownership: the session
        // dict and the key string are +1 (Create/Copy) and released here; the
        // dictionary value is borrowed and must not be released.
        unsafe {
            let dict = CGSessionCopyCurrentDictionary();
            if dict.is_null() {
                return true; // no session (loginwindow) — treat as locked
            }
            let Ok(key_c) = CString::new("CGSSessionScreenIsLocked") else {
                CFRelease(dict);
                return false;
            };
            let key = CFStringCreateWithCString(
                std::ptr::null(),
                key_c.as_ptr(),
                KCF_STRING_ENCODING_UTF8,
            );
            if key.is_null() {
                CFRelease(dict);
                return false;
            }
            let value = CFDictionaryGetValue(dict, key); // borrowed
            let locked = if value.is_null() {
                false
            } else {
                let tid = CFGetTypeID(value);
                if tid == CFBooleanGetTypeID() {
                    CFBooleanGetValue(value) != 0
                } else if tid == CFNumberGetTypeID() {
                    let mut n: i32 = 0;
                    CFNumberGetValue(value, KCF_NUMBER_SINT32, &mut n as *mut i32 as *mut c_void);
                    n != 0
                } else {
                    false
                }
            };
            CFRelease(key);
            CFRelease(dict);
            locked
        }
    }
}
