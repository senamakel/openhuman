use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WebChannelEvent {
    pub event: String,
    pub client_id: String,
    pub thread_id: String,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_response: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
}

static EVENT_BUS: Lazy<broadcast::Sender<WebChannelEvent>> = Lazy::new(|| {
    let (tx, _rx) = broadcast::channel(512);
    tx
});

pub fn subscribe() -> broadcast::Receiver<WebChannelEvent> {
    EVENT_BUS.subscribe()
}

pub fn publish(event: WebChannelEvent) {
    let _ = EVENT_BUS.send(event);
}
