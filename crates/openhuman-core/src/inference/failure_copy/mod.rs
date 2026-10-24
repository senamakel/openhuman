//! One host copy table for user-facing failure text.
//!
//! - [`table`]: failure class -> (`error_type` wire token, `source`,
//!   `retryable`, user copy), consumed by `web_chat::web_errors` to build the
//!   `chat_error` envelope.
//!
//! Provider-neutral and host-specific error copy used by the chat error surface.

mod table;

pub(crate) use table::{failure_copy, FailureClass};
