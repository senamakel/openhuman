//! Human-in-the-loop — formalize "request plan review" / "ask for confirmation"
//! as explicit graph nodes (issue #4249, item 5).
//!
//! A node pauses the run by returning
//! [`Command::Interrupt`](crate::openhuman::agent_graph::types::Command::Interrupt)
//! with an [`InterruptRequest`]. The executor persists a paused checkpoint; a
//! later resume folds the human's answer into the state via [`ApplyResume`] and
//! continues from the recorded `resume_to` node.
//!
//! This composes the existing approval surface rather than replacing it: a
//! production approval node can consult
//! [`crate::openhuman::approval`] and only `Interrupt` when a decision is
//! genuinely required.

use crate::openhuman::agent_graph::types::InterruptRequest;

/// Build an approval interrupt (yes/no, or custom options).
pub fn approval(question: impl Into<String>, options: Vec<String>) -> InterruptRequest {
    InterruptRequest {
        kind: "approval".to_string(),
        question: question.into(),
        options: if options.is_empty() {
            vec!["approve".to_string(), "reject".to_string()]
        } else {
            options
        },
        resume_to: None,
    }
}

/// Build a free-form clarification interrupt.
pub fn clarification(question: impl Into<String>) -> InterruptRequest {
    InterruptRequest {
        kind: "clarification".to_string(),
        question: question.into(),
        options: vec![],
        resume_to: None,
    }
}

/// Implemented by graph states that can absorb a human's resume input.
///
/// Called by the resume path (RPC `agent_graph_resume`) after restoring the
/// paused state and before continuing execution. The implementation decides how
/// the answer routes the rest of the graph (e.g. set an `approved: bool` field
/// a downstream conditional edge reads).
pub trait ApplyResume {
    /// Fold the human's `input` into the state. `input` is the raw answer (an
    /// option label, free text, or a JSON blob — the state decides how to parse).
    fn apply_resume(&mut self, input: &str);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_defaults_options() {
        let req = approval("ship it?", vec![]);
        assert_eq!(req.kind, "approval");
        assert_eq!(req.options, vec!["approve", "reject"]);
    }

    #[test]
    fn clarification_has_no_options() {
        let req = clarification("which file?");
        assert_eq!(req.kind, "clarification");
        assert!(req.options.is_empty());
    }

    #[test]
    fn apply_resume_folds_input() {
        struct S {
            approved: bool,
        }
        impl ApplyResume for S {
            fn apply_resume(&mut self, input: &str) {
                self.approved = input.eq_ignore_ascii_case("approve");
            }
        }
        let mut s = S { approved: false };
        s.apply_resume("approve");
        assert!(s.approved);
    }
}
