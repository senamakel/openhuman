//! Portable provider exports.
//!
//! Provider transports, and the provider-independent remote control and
//! in-chat approvals that used to be Telegram glue, belong to `tinychannels`.
//! [`relay`] is the exception: not a transport, but the host-side entry for
//! messages a gateway relays in from a hosted platform.

pub mod relay;

pub use tinychannels::providers::email_channel;
pub use tinychannels::providers::lark;
#[cfg(feature = "whatsapp-web")]
pub use tinychannels::providers::whatsapp_web;
pub use tinychannels::providers::{
    dingtalk, discord, imessage, irc, linq, mattermost, qq, signal, slack, telegram, whatsapp,
    yuanbao,
};
