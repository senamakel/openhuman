pub mod session_service;
pub mod socket_service;
pub mod tdlib;
pub mod tdlib_v8;

#[cfg(desktop)]
pub mod notification_service;

// Local LLM inference - available on desktop and Android (not iOS)
#[cfg(not(target_os = "ios"))]
pub mod llama;
