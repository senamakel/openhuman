//! Screen capture delegation to the shared accessibility middleware.

use chrono::Utc;

pub(crate) use crate::openhuman::accessibility::capture_screen_image_ref_for_context;
pub(crate) use crate::openhuman::accessibility::foreground_context;
pub(crate) use crate::openhuman::accessibility::AppContext;
pub(crate) use crate::openhuman::accessibility::CaptureMode;

pub(crate) fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}
