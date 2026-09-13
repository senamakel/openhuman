//! Serde DTOs for the TinyHumans backend auth and Socket.IO surfaces.
//!
//! - [`auth`] — `Session`, `User` (camelCase backend profile fields),
//!   `AuthErrorResponse` mirror `tinyhumansai/backend` payloads and must track
//!   its JSON; `AuthState` is the shape the core emits to the frontend.
//! - [`socket`] — `ConnectionStatus` and `SocketState` are the realtime
//!   connection state emitted to the frontend (consumed by
//!   `crate::platform::socket`); `SocketMessage` and the JSON-RPC 2.0 MCP
//!   envelope types `McpRequest` / `McpResponse` / `McpError` mirror backend
//!   payloads.
//!
//! Several of these are `#[allow(dead_code)]` and kept only as wire shapes.
//! Route implementations belong in `vendor/tinyhumans-sdk`, not here.

pub mod auth;
pub mod socket;
