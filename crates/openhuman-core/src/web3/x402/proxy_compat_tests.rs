use super::*;

use crate::config::ProxyScope;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn responder() -> (std::net::SocketAddr, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = crate::core::runtime::spawn_scoped(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            let read = stream.read(&mut bytes).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&bytes[..read]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
        String::from_utf8(request).unwrap()
    });
    (addr, task)
}

fn builder() -> reqwest13::ClientBuilder {
    // Isolate these local fixtures from any operator proxy environment.
    reqwest13::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(2))
}

#[tokio::test]
async fn selected_http_proxy_receives_the_request_instead_of_the_destination() {
    for all in [false, true] {
        let (proxy, served) = responder().await;
        let mut config = ProxyConfig {
            enabled: true,
            scope: ProxyScope::Services,
            services: vec!["tool.x402_request".into()],
            ..Default::default()
        };
        if all {
            config.all_proxy = Some(format!("http://{proxy}"));
        } else {
            config.http_proxy = Some(format!("http://{proxy}"));
        }
        let client = apply(
            builder().resolve("destination.invalid", "127.0.0.1:9".parse().unwrap()),
            "tool.x402_request",
            &config,
        )
        .build()
        .unwrap();
        let response = client
            .get("http://destination.invalid/check")
            .send()
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
        assert!(served
            .await
            .unwrap()
            .starts_with("GET http://destination.invalid/check HTTP/1.1"));
    }
}

#[tokio::test]
async fn unmatched_service_and_no_proxy_bypass_use_the_pinned_destination() {
    for bypass in [false, true] {
        let (destination, served) = responder().await;
        let config = ProxyConfig {
            enabled: true,
            scope: ProxyScope::Services,
            services: vec![if bypass {
                "tool.x402_request"
            } else {
                "provider.openai"
            }
            .into()],
            all_proxy: Some("http://127.0.0.1:1".into()),
            no_proxy: if bypass {
                vec!["destination.invalid".into()]
            } else {
                vec![]
            },
            ..Default::default()
        };
        let client = apply(
            builder().resolve("destination.invalid", destination),
            "tool.x402_request",
            &config,
        )
        .build()
        .unwrap();
        assert_eq!(
            client
                .get("http://destination.invalid/check")
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "ok"
        );
        assert!(served.await.unwrap().starts_with("GET /check HTTP/1.1"));
    }
}

#[test]
fn https_proxy_and_invalid_proxy_settings_preserve_build_behavior() {
    let mut config = ProxyConfig {
        enabled: true,
        https_proxy: Some("http://127.0.0.1:1".into()),
        ..Default::default()
    };
    assert!(apply(builder(), "tool.x402_request", &config)
        .build()
        .is_ok());
    config.https_proxy = Some("http://[broken".into());
    assert!(apply(builder(), "tool.x402_request", &config)
        .build()
        .is_ok());
}
