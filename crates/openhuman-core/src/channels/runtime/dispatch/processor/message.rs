//! The runtime's internal message envelope, wrapping a legacy
//! [`traits::ChannelMessage`] with the original TinyChannels inbound
//! envelope (when the message arrived via the relay transport).

use crate::channels::traits;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeChannelMessage {
    pub(crate) message: traits::ChannelMessage,
    pub(crate) inbound_envelope: Option<tinychannels::ChannelInboundEnvelope>,
}

impl RuntimeChannelMessage {
    pub(crate) fn with_inbound_envelope(
        message: traits::ChannelMessage,
        inbound_envelope: tinychannels::ChannelInboundEnvelope,
    ) -> Self {
        Self {
            message,
            inbound_envelope: Some(inbound_envelope),
        }
    }
}

impl From<traits::ChannelMessage> for RuntimeChannelMessage {
    fn from(message: traits::ChannelMessage) -> Self {
        Self {
            message,
            inbound_envelope: None,
        }
    }
}
