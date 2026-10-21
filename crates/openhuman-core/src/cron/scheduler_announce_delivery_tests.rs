use super::*;
use async_trait::async_trait;
use std::sync::Mutex;
use tinychannels_bus::{Channel, ChannelMessage, SendMessage};
use tinyflows_bus::schedule::types::DeliveryStatus;
use tokio::sync::mpsc;

type RecordedSends = Arc<Mutex<Vec<(String, Option<String>)>>>;

struct AnnounceMockChannel {
    name: String,
    sends: RecordedSends,
    fail: bool,
}

#[async_trait]
impl Channel for AnnounceMockChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        self.sends
            .lock()
            .unwrap()
            .push((message.recipient.clone(), message.idempotency_key.clone()));
        if self.fail {
            anyhow::bail!("mock announce send failed");
        }
        Ok(())
    }

    async fn listen(&self, _tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        Ok(())
    }
}

fn install_mock_channel(name: String, fail: bool) -> RecordedSends {
    let sends = Arc::new(Mutex::new(Vec::new()));
    let channel: Arc<dyn Channel> = Arc::new(AnnounceMockChannel {
        name: name.clone(),
        sends: Arc::clone(&sends),
        fail,
    });
    crate::cron::channel_bridge::register_channel_bridge(
        Arc::new(Mutex::new(std::collections::HashMap::new())),
        Arc::new(std::collections::HashMap::from([(name, channel)])),
        crate::cron::channel_bridge::MAX_HISTORY_MESSAGES,
    );
    sends
}

#[tokio::test]
async fn announce_to_unavailable_channel_is_reported_as_failed() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp).await;
    let missing_channel = format!("missing-{}", uuid::Uuid::new_v4());
    let mut job = test_job("echo ok");
    job.delivery = DeliveryConfig {
        mode: "announce".into(),
        channel: Some(missing_channel),
        to: Some("target".into()),
        best_effort: false,
    };

    let result = deliver_run(
        &crate::core::runtime::DispatchCtx::ambient(),
        &config,
        &job,
        "run-1",
        "hello",
        true,
    )
    .await;

    assert_eq!(
        result.unwrap(),
        DeliveryStatus::Failed,
        "an announce without a registered channel must not report Delivered"
    );
}

#[tokio::test]
async fn announce_send_failure_is_reported_as_failed() {
    let _bridge_lock = crate::cron::channel_bridge::TEST_BRIDGE_LOCK.lock().await;
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp).await;
    let channel = format!("failing-{}", uuid::Uuid::new_v4());
    let sends = install_mock_channel(channel.clone(), true);
    let mut job = test_job("echo ok");
    job.delivery = DeliveryConfig {
        mode: "announce".into(),
        channel: Some(channel),
        to: Some("target".into()),
        best_effort: false,
    };

    let result = deliver_run(
        &crate::core::runtime::DispatchCtx::ambient(),
        &config,
        &job,
        "run-2",
        "hello",
        true,
    )
    .await
    .unwrap();

    assert_eq!(result, DeliveryStatus::Failed);
    assert_eq!(sends.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn announce_success_is_reported_and_uses_a_run_idempotency_key() {
    let _bridge_lock = crate::cron::channel_bridge::TEST_BRIDGE_LOCK.lock().await;
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp).await;
    let channel = format!("working-{}", uuid::Uuid::new_v4());
    let sends = install_mock_channel(channel.clone(), false);
    let mut job = test_job("echo ok");
    job.delivery = DeliveryConfig {
        mode: "announce".into(),
        channel: Some(channel),
        to: Some("target".into()),
        best_effort: false,
    };

    let result = deliver_run(
        &crate::core::runtime::DispatchCtx::ambient(),
        &config,
        &job,
        "run-3",
        "hello",
        true,
    )
    .await
    .unwrap();

    assert_eq!(result, DeliveryStatus::Delivered);
    assert_eq!(
        *sends.lock().unwrap(),
        vec![("target".into(), Some(format!("cron:{}:run-3", job.id)))]
    );
}
