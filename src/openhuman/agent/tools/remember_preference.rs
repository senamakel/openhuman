//! Tool: remember_preference — deterministically pin an explicit user preference.
//!
//! Unlike the inference-based `UserProfileHook`, this tool is called by the
//! model when the user **explicitly** states or requests that a preference be
//! saved ("I prefer pnpm", "always be terse", "remember my timezone is IST").
//! The model supplies the structured `(class, key, value)` triple; the tool
//! writes it directly to the `user_profile` memory namespace as a pinned entry.
//!
//! # Storage contract
//!
//! Entries are keyed as `pinned/{class}/{key}` inside the `user_profile`
//! namespace, and the stored content is formatted as:
//!
//! ```text
//! [pinned] (class=tooling) package_manager: pnpm
//! ```
//!
//! This format is intentionally stable so `fetch_learned_context` can surface
//! the entries unchanged in the `UserProfileSection` prompt block.  The
//! `[pinned]` marker makes pinned entries visually distinct in the rendered
//! prompt.
//!
//! # Idempotency
//!
//! `Memory::store` performs an upsert (the underlying SQLite backend has a
//! `UNIQUE` constraint on `(namespace, key)` with `ON CONFLICT REPLACE`).
//! Re-saving the same `(class, key)` with a new value overwrites the previous
//! entry — no duplicates are created.
//!
//! # Bypassing the inference stack
//!
//! This tool does **not** touch the stability detector, the candidate
//! ring-buffer, `learning/extract/heuristics.rs`, or any other inference
//! component.  The preference is authoritative from the moment the tool
//! returns `Ok`.

use crate::openhuman::memory::ops::guard::active_memory_guard;
use crate::openhuman::security::policy::ToolOperation;
use crate::openhuman::security::SecurityPolicy;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tinymemory_api::provider::MemoryCore;
use tinymemory_api::types::{MemoryCategory, MemoryTaint};

/// Valid facet classes that a pinned preference may belong to.
///
/// The six classes provide a lightweight taxonomy that lets the system-prompt
/// renderer group or filter preferences in the future without requiring a
/// schema migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacetClass {
    /// Communication style (tone, verbosity, format).
    Style,
    /// Personal identity facts (name, role, pronouns).
    Identity,
    /// Toolchain choices (language, package manager, editor).
    Tooling,
    /// Hard vetoes — things the model must never do.
    Veto,
    /// Long-term goals and working objectives.
    Goal,
    /// Channel / communication-medium preferences.
    Channel,
}

impl FacetClass {
    /// Parse a case-insensitive string from the model's `class` argument.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "style" => Some(Self::Style),
            "identity" => Some(Self::Identity),
            "tooling" => Some(Self::Tooling),
            "veto" => Some(Self::Veto),
            "goal" => Some(Self::Goal),
            "channel" => Some(Self::Channel),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Style => "style",
            Self::Identity => "identity",
            Self::Tooling => "tooling",
            Self::Veto => "veto",
            Self::Goal => "goal",
            Self::Channel => "channel",
        }
    }
}

/// The namespace used for all pinned preferences in the memory backend.
pub const PINNED_PREFERENCES_NAMESPACE: &str = "user_profile";

/// Builds the memory key for a pinned preference.
///
/// Format: `pinned/{class}/{key}`, e.g. `pinned/tooling/package_manager`.
pub fn pinned_key(class: FacetClass, key: &str) -> String {
    format!("pinned/{}/{}", class.as_str(), key)
}

/// Builds the stored content string for a pinned preference.
///
/// Format: `[pinned] (class=tooling) package_manager: pnpm`
pub fn pinned_content(class: FacetClass, key: &str, value: &str) -> String {
    format!("[pinned] (class={}) {}: {}", class.as_str(), key, value)
}

/// Agent tool that explicitly pins a user preference into the `user_profile`
/// memory namespace.
///
/// The model calls this when the user states or requests a preference be
/// remembered.  All arguments (`class`, `key`, `value`) are supplied by the
/// model — it maps the user's natural-language intent to the structured triple.
pub struct RememberPreferenceTool {
    security: Arc<SecurityPolicy>,
}

impl RememberPreferenceTool {
    /// Holds no memory handle — the guarded driver is resolved per call.
    #[must_use]
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for RememberPreferenceTool {
    fn name(&self) -> &str {
        "remember_preference"
    }

    fn description(&self) -> &str {
        "Pin an explicit user preference so it persists across all future sessions. \
         Call this when the user states or asks to save a preference — e.g. \
         \"I prefer pnpm\", \"always be terse\", \"never email Sarah\", \
         \"remember my timezone is IST\". \
         Map the user's intent to a `class` (one of: style, identity, tooling, veto, goal, channel), \
         a snake_case `key` (e.g. package_manager, verbosity, timezone), \
         and a concise `value` string. \
         Re-saving the same class+key overwrites the previous value — no duplicates are created."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["class", "key", "value"],
            "properties": {
                "class": {
                    "type": "string",
                    "enum": ["style", "identity", "tooling", "veto", "goal", "channel"],
                    "description": "Facet class — style (tone/format), identity (name/role), \
                                   tooling (language/editor/package manager), \
                                   veto (hard do-not-do), goal (objective), \
                                   channel (communication medium preference)."
                },
                "key": {
                    "type": "string",
                    "description": "Snake_case slug that uniquely names this preference within its class, \
                                   e.g. package_manager, verbosity, timezone, preferred_language. \
                                   Must contain only lowercase letters, digits, and underscores."
                },
                "value": {
                    "type": "string",
                    "description": "The preference value, e.g. pnpm, terse, IST, Rust."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        tracing::debug!(
            "[tool][remember_preference] invoked with args: class={:?} key={:?} value_present={} value_len={}",
            args.get("class").and_then(|v| v.as_str()),
            args.get("key").and_then(|v| v.as_str()),
            args.get("value").is_some(),
            args.get("value")
                .and_then(|v| v.as_str())
                .map_or(0, |s| s.len()),
        );

        // Security gate — tool requires Write-level autonomy.
        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "remember_preference")
        {
            tracing::warn!("[tool][remember_preference] security gate rejected: {error}");
            return Ok(ToolResult::error(error));
        }

        // Parse class.
        let class_str = match args.get("class").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return Ok(ToolResult::error(
                    "missing required argument: class".to_string(),
                ));
            }
        };
        let class = match FacetClass::parse(class_str) {
            Some(c) => c,
            None => {
                tracing::warn!(
                    "[tool][remember_preference] invalid class={:?}; valid values: \
                     style, identity, tooling, veto, goal, channel",
                    class_str
                );
                return Ok(ToolResult::error(format!(
                    "invalid class {:?}; must be one of: style, identity, tooling, veto, goal, channel",
                    class_str
                )));
            }
        };

        // Parse key — must be a non-empty, snake_case slug.
        let key = match args.get("key").and_then(|v| v.as_str()) {
            Some(k) => k.trim(),
            None => {
                return Ok(ToolResult::error(
                    "missing required argument: key".to_string(),
                ));
            }
        };
        if key.is_empty() {
            return Ok(ToolResult::error("key cannot be empty".to_string()));
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            tracing::warn!(
                "[tool][remember_preference] key {:?} contains invalid characters; \
                 only lowercase letters, digits, and underscores are allowed (snake_case)",
                key
            );
            return Ok(ToolResult::error(format!(
                "key {:?} contains invalid characters; use only lowercase letters, digits, and underscores (snake_case)",
                key
            )));
        }

        // Parse value — normalize to a single line so that embedded \r/\n cannot
        // corrupt the line-oriented `[pinned] … key: value` storage format.
        let value_raw = match args.get("value").and_then(|v| v.as_str()) {
            Some(v) => v,
            None => {
                return Ok(ToolResult::error(
                    "missing required argument: value".to_string(),
                ));
            }
        };
        // Collapse any embedded CR/LF to a single space, then trim surrounding
        // whitespace so the stored and pinned representations are always one line.
        let value_normalized: String = value_raw
            .replace(['\r', '\n'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let value = value_normalized.as_str();
        if value.is_empty() {
            return Ok(ToolResult::error("value cannot be empty".to_string()));
        }

        let mem_key = pinned_key(class, key);
        let content = pinned_content(class, key, value);

        tracing::debug!(
            "[tool][remember_preference] upserting pinned preference: namespace={} key={} class={} value_len={}",
            PINNED_PREFERENCES_NAMESPACE,
            mem_key,
            class.as_str(),
            value.len()
        );

        let guard = active_memory_guard()
            .await
            .map_err(|e| anyhow::anyhow!("remember_preference: {e}"))?;
        match guard
            .store(
                PINNED_PREFERENCES_NAMESPACE,
                &mem_key,
                &content,
                // Core category — pinned preferences are permanent user facts.
                MemoryCategory::Core,
                None,
                // Requested provenance; the guard stamps the effective value.
                MemoryTaint::default(),
            )
            .await
        {
            Ok(()) => {
                tracing::info!(
                    "[tool][remember_preference] pinned preference stored: \
                     namespace={} key={} class={} value_len={}",
                    PINNED_PREFERENCES_NAMESPACE,
                    mem_key,
                    class.as_str(),
                    value.len()
                );
                Ok(ToolResult::success(format!(
                    "Preference saved: [{class}] {key} = {value}",
                    class = class.as_str()
                )))
            }
            Err(e) => {
                tracing::error!(
                    "[tool][remember_preference] failed to store preference \
                     namespace={} key={}: {e:#}",
                    PINNED_PREFERENCES_NAMESPACE,
                    mem_key
                );
                Ok(ToolResult::error(format!("Failed to save preference: {e}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::security::{AutonomyLevel, SecurityPolicy};
    use serde_json::json;
    use tinymemory_api::types::MemoryEntry;

    fn test_security() -> Arc<SecurityPolicy> {
        Arc::new(SecurityPolicy::default())
    }

    /// Read a pinned preference back through the guard — the door the tool
    /// wrote it through.
    ///
    /// The tool holds no handle: it resolves the *bound* driver per call, so a
    /// fixture store built here would be a different store entirely and could
    /// never see the write. That matters most for the absence assertions —
    /// against a store the tool never writes to they hold whether or not the
    /// refusal under test worked, which is worse than no assertion at all.
    async fn stored(key: &str) -> Option<MemoryEntry> {
        active_memory_guard()
            .await
            .expect("a bound memory guard")
            .get(PINNED_PREFERENCES_NAMESPACE, key)
            .await
            .expect("read back through the guard")
    }

    /// How many rows the guard holds under `key`.
    ///
    /// Filtered by exact key rather than counting the namespace: the pinned
    /// namespace is a fixed constant the tool derives, so every test in the
    /// process writes into the same one and a namespace-wide count would be a
    /// function of what else ran.
    async fn stored_row_count(key: &str) -> usize {
        active_memory_guard()
            .await
            .expect("a bound memory guard")
            .list(Some(PINNED_PREFERENCES_NAMESPACE), None, None)
            .await
            .expect("list through the guard")
            .into_iter()
            .filter(|entry| entry.key == key)
            .count()
    }

    // ── FacetClass ─────────────────────────────────────────────────────────

    #[test]
    fn facet_class_parse_case_insensitive() {
        assert_eq!(FacetClass::parse("Style"), Some(FacetClass::Style));
        assert_eq!(FacetClass::parse("IDENTITY"), Some(FacetClass::Identity));
        assert_eq!(FacetClass::parse("tooling"), Some(FacetClass::Tooling));
        assert_eq!(FacetClass::parse("veto"), Some(FacetClass::Veto));
        assert_eq!(FacetClass::parse("goal"), Some(FacetClass::Goal));
        assert_eq!(FacetClass::parse("channel"), Some(FacetClass::Channel));
        assert_eq!(FacetClass::parse("unknown"), None);
        assert_eq!(FacetClass::parse(""), None);
    }

    #[test]
    fn facet_class_as_str_round_trips() {
        for class in [
            FacetClass::Style,
            FacetClass::Identity,
            FacetClass::Tooling,
            FacetClass::Veto,
            FacetClass::Goal,
            FacetClass::Channel,
        ] {
            let parsed = FacetClass::parse(class.as_str()).expect("round-trip must succeed");
            assert_eq!(parsed, class);
        }
    }

    // ── Key / content helpers ───────────────────────────────────────────────

    #[test]
    fn pinned_key_format() {
        assert_eq!(
            pinned_key(FacetClass::Tooling, "package_manager"),
            "pinned/tooling/package_manager"
        );
        assert_eq!(
            pinned_key(FacetClass::Style, "verbosity"),
            "pinned/style/verbosity"
        );
    }

    #[test]
    fn pinned_content_format() {
        assert_eq!(
            pinned_content(FacetClass::Tooling, "package_manager", "pnpm"),
            "[pinned] (class=tooling) package_manager: pnpm"
        );
    }

    // ── Tool metadata ───────────────────────────────────────────────────────

    #[test]
    fn tool_name_and_permission() {
        let tool = RememberPreferenceTool::new(test_security());
        assert_eq!(tool.name(), "remember_preference");
        assert_eq!(tool.permission_level(), PermissionLevel::Write);
    }

    #[test]
    fn schema_has_required_fields() {
        let tool = RememberPreferenceTool::new(test_security());
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().unwrap();
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"class"));
        assert!(names.contains(&"key"));
        assert!(names.contains(&"value"));
    }

    // ── Argument validation ─────────────────────────────────────────────────

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn missing_class_returns_error() {
        let tool = RememberPreferenceTool::new(test_security());
        let result = tool
            .execute(json!({"key": "timezone", "value": "IST"}))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output().contains("class"));
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn invalid_class_returns_error() {
        let tool = RememberPreferenceTool::new(test_security());
        let result = tool
            .execute(json!({"class": "bogus", "key": "timezone", "value": "IST"}))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output().contains("invalid class"));
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn missing_key_returns_error() {
        let tool = RememberPreferenceTool::new(test_security());
        let result = tool
            .execute(json!({"class": "style", "value": "terse"}))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output().contains("key"));
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn empty_key_returns_error() {
        let tool = RememberPreferenceTool::new(test_security());
        let result = tool
            .execute(json!({"class": "style", "key": "   ", "value": "terse"}))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output().contains("key cannot be empty"));
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn key_with_spaces_returns_error() {
        let tool = RememberPreferenceTool::new(test_security());
        let result = tool
            .execute(json!({"class": "style", "key": "my pref", "value": "terse"}))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output().contains("invalid characters"));
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn missing_value_returns_error() {
        let tool = RememberPreferenceTool::new(test_security());
        let result = tool
            .execute(json!({"class": "tooling", "key": "pkg_mgr"}))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.output().contains("value"));
    }

    // ── Successful upsert ───────────────────────────────────────────────────

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn stores_preference_in_user_profile_namespace() {
        let tool = RememberPreferenceTool::new(test_security());
        let result = tool
            .execute(json!({"class": "tooling", "key": "package_manager", "value": "pnpm"}))
            .await
            .unwrap();
        assert!(!result.is_error, "unexpected error: {}", result.output());
        assert!(result.output().contains("package_manager"));

        let entry = stored("pinned/tooling/package_manager").await;
        assert!(entry.is_some(), "entry must have been stored");
        let entry = entry.unwrap();
        assert_eq!(
            entry.content,
            "[pinned] (class=tooling) package_manager: pnpm"
        );
        assert_eq!(entry.category, MemoryCategory::Core);
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn idempotent_overwrite_does_not_create_duplicate() {
        let tool = RememberPreferenceTool::new(test_security());

        // First write.
        tool.execute(json!({"class": "style", "key": "verbosity", "value": "verbose"}))
            .await
            .unwrap();

        // Overwrite with new value.
        let result = tool
            .execute(json!({"class": "style", "key": "verbosity", "value": "terse"}))
            .await
            .unwrap();
        assert!(
            !result.is_error,
            "overwrite must succeed: {}",
            result.output()
        );

        // Verify the overwritten content via get() which reads the actual content column.
        let entry = stored("pinned/style/verbosity")
            .await
            .expect("entry must exist after overwrite");
        assert_eq!(
            entry.content, "[pinned] (class=style) verbosity: terse",
            "overwritten content must reflect the latest value"
        );

        // Verify no duplicate entries exist via list().
        assert_eq!(
            stored_row_count("pinned/style/verbosity").await,
            1,
            "must not duplicate entries"
        );
    }

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn stores_all_six_classes() {
        let tool = RememberPreferenceTool::new(test_security());

        for (class, key, value) in [
            ("style", "tone", "formal"),
            ("identity", "name", "Alice"),
            ("tooling", "editor", "neovim"),
            ("veto", "no_emoji", "true"),
            ("goal", "ship_feature", "memory refactor"),
            ("channel", "preferred", "slack"),
        ] {
            let result = tool
                .execute(json!({"class": class, "key": key, "value": value}))
                .await
                .unwrap();
            assert!(
                !result.is_error,
                "class={class} failed: {}",
                result.output()
            );
        }

        // Asserted key by key rather than as a namespace count: the pinned
        // namespace is a fixed constant, so every test in the process writes
        // into the same one and a total would be a function of what else ran.
        for key in [
            "pinned/style/tone",
            "pinned/identity/name",
            "pinned/tooling/editor",
            "pinned/veto/no_emoji",
            "pinned/goal/ship_feature",
            "pinned/channel/preferred",
        ] {
            assert!(stored(key).await.is_some(), "{key} must have been stored");
        }
    }

    // ── Security gate ───────────────────────────────────────────────────────

    #[tokio::test]
    #[ignore = "needs a built tinymemory module (OPENHUMAN_MODULE_PATH) and its own process: \
the tool resolves the bound driver rather than being handed a memory handle"]
    async fn blocked_in_readonly_mode() {
        let readonly = Arc::new(SecurityPolicy {
            autonomy: AutonomyLevel::ReadOnly,
            ..SecurityPolicy::default()
        });
        let tool = RememberPreferenceTool::new(readonly);
        // A key no other test writes: the absence assertion now runs against
        // the shared bound store, so `stores_all_six_classes`' `style/tone`
        // would satisfy it for the wrong reason if the two collided.
        let result = tool
            .execute(json!({"class": "style", "key": "readonly_tone", "value": "formal"}))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(stored("pinned/style/readonly_tone").await.is_none());
    }
}
