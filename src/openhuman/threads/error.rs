//! Error taxonomy for the conversation threads RPC surface.

/// Stable JSON-RPC discriminator used by the frontend to handle stale thread
/// references without string matching or user-facing error toasts.
pub const THREAD_NOT_FOUND_KIND: &str = "ThreadNotFound";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThreadsError {
    #[error("thread {thread_id} not found")]
    NotFound { thread_id: String },
    #[error("{0}")]
    Message(String),
}

impl ThreadsError {
    pub fn not_found(thread_id: impl Into<String>) -> Self {
        Self::NotFound {
            thread_id: thread_id.into(),
        }
    }

    pub fn from_thread_scoped_store_error(thread_id: &str, err: String) -> Self {
        if parse_thread_not_found_message(&err).is_some() {
            Self::not_found(thread_id)
        } else {
            Self::Message(err)
        }
    }
}

impl From<String> for ThreadsError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

impl From<&str> for ThreadsError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<ThreadsError> for String {
    fn from(value: ThreadsError) -> Self {
        value.to_string()
    }
}

/// Parse the canonical display form (`thread <id> not found`) and the legacy
/// store form (`thread <id> does not exist`) so the RPC boundary can classify
/// both upgraded and in-flight older call paths.
pub fn parse_thread_not_found_message(message: &str) -> Option<&str> {
    let thread_id = message
        .strip_prefix("thread ")
        .and_then(|rest| rest.strip_suffix(" not found"))
        .or_else(|| {
            message
                .strip_prefix("thread ")
                .and_then(|rest| rest.strip_suffix(" does not exist"))
        })?;
    if thread_id.trim().is_empty() {
        None
    } else {
        Some(thread_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_display_is_stable() {
        let err = ThreadsError::not_found("thread-123");
        assert_eq!(err.to_string(), "thread thread-123 not found");
    }

    #[test]
    fn parses_canonical_and_legacy_thread_not_found_messages() {
        assert_eq!(
            parse_thread_not_found_message("thread thread-123 not found"),
            Some("thread-123")
        );
        assert_eq!(
            parse_thread_not_found_message("thread thread-123 does not exist"),
            Some("thread-123")
        );
        assert_eq!(
            parse_thread_not_found_message("message msg-1 not found in thread thread-123"),
            None
        );
    }
}
