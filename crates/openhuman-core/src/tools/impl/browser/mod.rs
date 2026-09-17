//! Browser tools: DOM-snapshot automation (`BrowserTool` with pluggable
//! backends), `BrowserOpenTool`, and `ImageInfoTool`. Gates and the host
//! allowlist rules are documented in `tools/impl/README.md`.

#[allow(clippy::module_inception)]
mod browser;
mod browser_open;
mod image_info;
mod playwright_backend;

pub use browser::{BrowserAction, BrowserTool, ComputerUseConfig};
pub use browser_open::BrowserOpenTool;
pub use image_info::ImageInfoTool;
