//! Shared MCP client + registry for remote MCP servers exposed to agents.
//!
//! This module is the client-side counterpart to `openhuman::mcp_server`.
//! It keeps track of named remote MCP servers, lists their tool surfaces,
//! and forwards `tools/call` requests through a small stateless HTTP client.

mod client;
mod registry;

pub use client::{
    redact_endpoint, AuthorizationServerMetadata, McpAuthChallenge, McpAuthorizationContext,
    McpHttpClient, McpInitializeResult, McpRemoteTool, McpServerToolResult, McpSseEvent,
    ProtectedResourceMetadata,
};
pub use registry::{McpRegistrySource, McpServerDefinition, McpServerRegistry};
