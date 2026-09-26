//! Linking the managed TinyHumans Telegram / Discord bots to the user's
//! TinyHumans account, on the SDK's typed `auth()` client.
//!
//! - `auth.create_channel_link_token` — `POST /auth/channels/{channel}/link-token`
//! - `channels.telegram_login_start` / `telegram_login_check`
//! - `channels.discord_link_start` / `discord_link_check`
//!
//! All five need a TinyHumans account (the link binds a bot conversation to
//! it), which is why they live here rather than in the core. Their RPC names
//! are unchanged wire contracts and share the `auth` / `channels` namespaces
//! with core controllers. The two `channels.*` pairs only exist when the
//! `channels` feature is on, exactly as before the move.

mod ops;
mod schemas;

pub use ops::*;
pub use schemas::{
    all_channel_link_controller_schemas, all_channel_link_registered_controllers,
    channel_link_schemas,
};
