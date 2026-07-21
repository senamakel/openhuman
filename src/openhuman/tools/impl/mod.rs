pub mod browser;
// The `computer` agent-tool family (ax_interact / automate / mouse / keyboard) is
// compiled out with the `desktop-automation` feature (#5049). Leaf gate: the tool
// registrations in `tools/ops.rs` carry matching `#[cfg]` so the tools are absent
// (not error-degraded) when off.
#[cfg(feature = "desktop-automation")]
pub mod computer;
pub mod document;
pub mod filesystem;
pub mod network;
pub mod presentation;
pub mod system;

pub use browser::*;
#[cfg(feature = "desktop-automation")]
pub use computer::*;
pub use document::DocumentTool;
pub use filesystem::*;
pub use network::*;
pub use presentation::PresentationTool;
pub use system::*;
