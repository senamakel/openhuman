//! Free `render_*` functions, sub-agent prompt renderer, and workspace-file
//! I/O helpers.
//!
//! The `render_*` family provides a functional interface over the section
//! structs in [`super::sections`] — `agents/<id>/prompt.rs` builders call
//! these to assemble their own final system prompt without needing the full
//! [`super::builder::SystemPromptBuilder`] machinery.
//!
//! Split by responsibility:
//! - [`section_renderers`] — the `render_*` wrappers, per-turn datetime stamp,
//!   ambient-environment composer, and memory date label.
//! - [`subagent`] — the narrow KV-cache-stable sub-agent prompt renderer.
//! - [`workspace_files`] — workspace-file seeding and prompt injection.

mod section_renderers;
mod subagent;
mod workspace_files;

pub use section_renderers::{
    current_datetime_line, memory_date_label, render_agents_md, render_ambient_environment,
    render_datetime, render_grounding, render_identity, render_runtime, render_safety,
    render_tools, render_user_files, render_user_identity, render_user_memory,
    render_user_reflections, render_workspace,
};
pub use subagent::{render_subagent_system_prompt, render_subagent_system_prompt_with_format};
pub(crate) use workspace_files::write_agents_md_blocks;
pub use workspace_files::{
    default_workspace_file_content, inject_inline_content, inject_snapshot_content,
    inject_workspace_file, inject_workspace_file_capped, sync_workspace_file,
};
