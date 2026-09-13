//! Where a sub-agent's system prompt comes from: inline text, a prompt
//! file, or a runtime [`PromptBuilder`] function (built-ins only).

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};

/// Builder function signature for [`PromptSource::Dynamic`]. Takes the
/// full runtime [`crate::agent::context::prompt::PromptContext`]
/// (tools, skills, memory, connected integrations, dispatcher, model,
/// …) and returns the final system prompt body — typically assembled
/// by calling the `render_*` section helpers in
/// [`crate::agent::context::prompt`] in the order the builder
/// wants.
pub type PromptBuilder =
    fn(&crate::agent::context::prompt::PromptContext<'_>) -> anyhow::Result<String>;

/// Where the sub-agent's core system prompt comes from.
#[derive(Clone)]
pub enum PromptSource {
    /// Inline prompt string (custom TOML-defined agents).
    Inline(String),
    /// Relative path under the workspace's `prompts/` directory or under
    /// `crates/openhuman-core/src/agent/prompts/` for built-ins. Resolved by the runner
    /// at spawn time.
    File { path: String },
    /// Function-driven prompt: the builder is invoked at spawn time with
    /// a [`crate::agent::context::prompt::PromptContext`] so the returned body can depend on runtime
    /// state (available tools, user profile, connected skills, etc.).
    ///
    /// Only constructed in-process (by built-in agent loaders). Not
    /// deserializable from TOML — TOML-authored agents must use `inline`
    /// or `file`.
    Dynamic(PromptBuilder),
}

impl std::fmt::Debug for PromptSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromptSource::Inline(s) => f.debug_tuple("Inline").field(&s).finish(),
            PromptSource::File { path } => f.debug_struct("File").field("path", path).finish(),
            PromptSource::Dynamic(_) => f.debug_tuple("Dynamic").field(&"<fn>").finish(),
        }
    }
}

impl Serialize for PromptSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            PromptSource::Inline(s) => map.serialize_entry("inline", s)?,
            PromptSource::File { path } => {
                #[derive(Serialize)]
                struct FileBody<'a> {
                    path: &'a str,
                }
                map.serialize_entry("file", &FileBody { path })?;
            }
            // Opaque marker — runtime-only. Round-trips back through
            // Deserialize would produce an error (Dynamic is unsupported
            // there) which is intentional: RPC consumers treat Dynamic
            // sources as "built-in, runtime-generated".
            PromptSource::Dynamic(_) => map.serialize_entry("dynamic", &serde_json::Value::Null)?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for PromptSource {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Shape {
            Inline(String),
            File { path: String },
        }
        Shape::deserialize(deserializer).map(|s| match s {
            Shape::Inline(body) => PromptSource::Inline(body),
            Shape::File { path } => PromptSource::File { path },
        })
    }
}
