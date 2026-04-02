//! Permission detection — delegates to accessibility middleware.

pub(crate) use crate::openhuman::accessibility::detect_permissions;
pub(crate) use crate::openhuman::accessibility::permission_to_str;
#[cfg(target_os = "macos")]
pub(crate) use crate::openhuman::accessibility::{
    detect_accessibility_permission, detect_input_monitoring_permission,
    detect_screen_recording_permission, open_macos_privacy_pane, request_accessibility_access,
    request_screen_recording_access,
};
