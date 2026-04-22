use serde::{Deserialize, Serialize};

/// Category used by the frontend notification center to apply per-category
/// preferences. Matches `NotificationCategory` in
/// `app/src/store/notificationSlice.ts` — keep the two in sync.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CoreNotificationCategory {
    Messages,
    Agents,
    Skills,
    System,
}

/// Wire payload emitted on the `core_notification` socket event. Short,
/// user-facing fields only — downstream UI shapes title/body/category into
/// its own notification item structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreNotificationEvent {
    /// Stable id used for de-duplication in the center (e.g.
    /// `"cron:<job_id>:<ts>"`). The frontend keys by this id so repeated
    /// publishes for the same logical event don't pile up.
    pub id: String,
    pub category: CoreNotificationCategory,
    pub title: String,
    pub body: String,
    /// Optional in-app deep link the user is sent to when they click the
    /// notification (mirrors the `deepLink` field on the frontend item).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep_link: Option<String>,
    /// Wall-clock milliseconds since the unix epoch at publish time.
    pub timestamp_ms: u64,
}
