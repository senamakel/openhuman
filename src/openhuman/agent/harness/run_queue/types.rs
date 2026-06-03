//! Queue model types for active-run steering and queue controls.

use serde::{Deserialize, Serialize};

/// What happens when a message arrives while an agent turn is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueMode {
    /// Abort the in-flight turn and start a fresh one (existing default).
    Interrupt,
    /// Inject the message into the running turn's history at the next
    /// model boundary, after the current tool-call batch completes.
    Steer,
    /// Queue the message to be delivered as a new user turn after the
    /// current turn completes naturally.
    Followup,
    /// Queue the message as additional context — injected alongside
    /// steers at the next model boundary, but prefixed so the model
    /// treats it as supplementary information rather than a new request.
    Collect,
}

impl Default for QueueMode {
    fn default() -> Self {
        Self::Interrupt
    }
}

impl QueueMode {
    /// Short stable label for telemetry and logging.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Interrupt => "interrupt",
            Self::Steer => "steer",
            Self::Followup => "followup",
            Self::Collect => "collect",
        }
    }

    /// Parse from a user-supplied string, returning `None` for unknown values.
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "interrupt" => Some(Self::Interrupt),
            "steer" => Some(Self::Steer),
            "followup" | "follow_up" => Some(Self::Followup),
            "collect" => Some(Self::Collect),
            _ => None,
        }
    }
}

/// One queued message waiting for delivery to a running agent turn.
#[derive(Debug, Clone)]
pub struct QueueEntry {
    /// Unique id (UUID) for this entry — used in telemetry events.
    pub id: String,
    /// The user message content.
    pub message: String,
    /// How this entry should be delivered.
    pub mode: QueueMode,
    /// Monotonic instant when the entry was enqueued (for latency tracking).
    pub enqueued_at: std::time::Instant,
    /// Socket.IO client id of the sender.
    pub client_id: String,
    /// Conversation thread id.
    pub thread_id: String,
}

/// Counts of pending entries per mode (steers, followups, collects).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueCounts {
    pub steers: usize,
    pub followups: usize,
    pub collects: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_mode_default_is_interrupt() {
        assert_eq!(QueueMode::default(), QueueMode::Interrupt);
    }

    #[test]
    fn queue_mode_as_str_roundtrips() {
        for mode in [
            QueueMode::Interrupt,
            QueueMode::Steer,
            QueueMode::Followup,
            QueueMode::Collect,
        ] {
            let s = mode.as_str();
            assert_eq!(
                QueueMode::from_str_opt(s),
                Some(mode),
                "roundtrip failed for {s}"
            );
        }
    }

    #[test]
    fn queue_mode_from_str_accepts_aliases() {
        assert_eq!(
            QueueMode::from_str_opt("follow_up"),
            Some(QueueMode::Followup)
        );
        assert_eq!(QueueMode::from_str_opt("STEER"), Some(QueueMode::Steer));
        assert_eq!(
            QueueMode::from_str_opt("INTERRUPT"),
            Some(QueueMode::Interrupt)
        );
    }

    #[test]
    fn queue_mode_from_str_rejects_unknown() {
        assert_eq!(QueueMode::from_str_opt("unknown"), None);
        assert_eq!(QueueMode::from_str_opt(""), None);
    }

    #[test]
    fn queue_mode_serde_roundtrip() {
        let mode = QueueMode::Steer;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"steer\"");
        let back: QueueMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mode);
    }
}
