//! Sentry envelope transport backed by the core's shared reqwest 0.12 stack.

use std::sync::{mpsc, Arc};
use std::time::Duration;

use sentry::{ClientOptions, Envelope, Transport};

enum Command {
    Send(Envelope),
    Flush(mpsc::SyncSender<()>),
    Shutdown(mpsc::SyncSender<()>),
}

/// A background Sentry transport that avoids Sentry's reqwest 0.13 dependency.
pub struct SharedReqwestTransport {
    sender: mpsc::Sender<Command>,
}

impl SharedReqwestTransport {
    /// Build a transport from the same client options consumed by Sentry's
    /// built-in HTTP transport.
    pub fn new(options: &ClientOptions) -> Self {
        let mut builder = reqwest::blocking::Client::builder();
        if options.accept_invalid_certs {
            builder = builder.danger_accept_invalid_certs(true);
        }
        if let Some(proxy) = options.http_proxy.as_ref() {
            if let Ok(proxy) = reqwest::Proxy::http(proxy.as_ref()) {
                builder = builder.proxy(proxy);
            }
        }
        if let Some(proxy) = options.https_proxy.as_ref() {
            if let Ok(proxy) = reqwest::Proxy::https(proxy.as_ref()) {
                builder = builder.proxy(proxy);
            }
        }
        let client = builder
            .build()
            .expect("reqwest 0.12 TLS client must be available");
        let dsn = options
            .dsn
            .as_ref()
            .expect("Sentry transport requires a DSN");
        let auth = dsn.to_auth(Some(&options.user_agent)).to_string();
        let url = dsn.envelope_api_url().to_string();
        let (sender, receiver) = mpsc::channel();

        std::thread::Builder::new()
            .name("sentry-transport".into())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    match command {
                        Command::Send(envelope) => {
                            let mut body = Vec::new();
                            if envelope.to_writer(&mut body).is_ok() {
                                let _ = client
                                    .post(&url)
                                    .header("X-Sentry-Auth", &auth)
                                    .body(body)
                                    .send();
                            }
                        }
                        Command::Flush(done) => {
                            let _ = done.send(());
                        }
                        Command::Shutdown(done) => {
                            let _ = done.send(());
                            break;
                        }
                    }
                }
            })
            .expect("spawn Sentry transport worker");

        Self { sender }
    }

    fn drain(&self, timeout: Duration, shutdown: bool) -> bool {
        let (done_tx, done_rx) = mpsc::sync_channel(0);
        let command = if shutdown {
            Command::Shutdown(done_tx)
        } else {
            Command::Flush(done_tx)
        };
        self.sender.send(command).is_ok() && done_rx.recv_timeout(timeout).is_ok()
    }
}

impl Transport for SharedReqwestTransport {
    fn send_envelope(&self, envelope: Envelope) {
        let _ = self.sender.send(Command::Send(envelope));
    }

    fn flush(&self, timeout: Duration) -> bool {
        self.drain(timeout, false)
    }

    fn shutdown(&self, timeout: Duration) -> bool {
        self.drain(timeout, true)
    }
}

/// Factory suitable for [`ClientOptions::transport`].
pub fn factory(options: &ClientOptions) -> Arc<dyn Transport> {
    Arc::new(SharedReqwestTransport::new(options))
}
