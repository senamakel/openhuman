//! Compile-time visibility into the `http-server` gate.
//!
//! Deliberately **ungated**: unlike the rest of the transport surface, this
//! module is compiled in both feature states, because its whole purpose is to
//! report which state the binary ended up in. It mirrors
//! [`crate::voice::VOICE_COMPILED_IN`] and
//! [`crate::inference::INFERENCE_COMPILED_IN`].

/// Whether the real HTTP + Socket.IO server transport was compiled into this
/// binary.
///
/// Cargo features are per-crate and invisible to dependents' `#[cfg]`, so a
/// consumer that *requires* the transport (the desktop shell, which reaches the
/// core only over `http://127.0.0.1:<port>/rpc`) has no other way to detect
/// that it silently got a slim build with no listener — exactly the class of
/// silent drop that shipped `voice` broken from v0.58.19 (#4901).
///
/// The shell asserts this at compile time (`const _: () = assert!(...)` in
/// `crates/openhuman-app/src/lib.rs`), turning that silent runtime failure (every RPC
/// unreachable — the frontend can't talk to a core that never bound a socket)
/// into a build failure. When `false`, the direct `socketioxide` dependency is
/// dropped from the graph (verify with `cargo tree -i socketioxide`); `axum`
/// stays linked transitively via `tinychannels`, so only the gated HTTP +
/// Socket.IO transport surface — not `axum` itself — leaves the slim build.
pub const HTTP_SERVER_COMPILED_IN: bool = cfg!(feature = "http-server");

#[cfg(test)]
#[path = "http_server_status_tests.rs"]
mod tests;
