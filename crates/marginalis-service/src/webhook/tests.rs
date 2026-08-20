//! Webhook HTTP adapterの結合試験。

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use marginalis_application::{WebhookDeliveryFailure, WebhookDeliverySender};
use marginalis_domain::UnixMillis;

use super::*;

#[derive(Clone, Default)]
struct Captured {
    requests: Arc<Mutex<Vec<(HeaderMap, String)>>>,
}

/// 127.0.0.1へ試験用の受信サーバーを立てる。応答は状態codeと本文を指定する。
async fn receiver(status: u16, body: &'static str) -> (SocketAddr, Captured) {
    let captured = Captured::default();
    let router = Router::new()
        .route(
            "/hook",
            post(
                move |State(captured): State<Captured>, headers: HeaderMap, request: String| async move {
                    captured
                        .requests
                        .lock()
                        .expect("capture lock")
                        .push((headers, request));
                    (
                        axum::http::StatusCode::from_u16(status).expect("status"),
                        body,
                    )
                },
            ),
        )
        .with_state(captured.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind receiver");
    let address = listener.local_addr().expect("receiver address");
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve receiver");
    });
    (address, captured)
}

fn loopback_sender() -> WebhookHttpSender {
    WebhookHttpSender::new(vec!["127.0.0.1".into()])
}

#[tokio::test]
async fn delivers_with_a_verifiable_signature() {
    let (address, captured) = receiver(200, "ok").await;
    let sender = loopback_sender();
    let url = format!("http://127.0.0.1:{}/hook", address.port());
    let sent_at = UnixMillis::new(1_700_000_000_000);
    sender
        .deliver(&url, "shared-secret", sent_at, r#"{"event_id":"abc"}"#)
        .await
        .expect("delivery succeeds");

    let requests = captured.requests.lock().expect("capture lock");
    let (headers, body) = &requests[0];
    assert_eq!(body, r#"{"event_id":"abc"}"#);
    assert_eq!(
        headers
            .get(WEBHOOK_TIMESTAMP_HEADER)
            .expect("timestamp header"),
        "1700000000000"
    );
    // 受信側と同じ計算で署名を再現できる。
    assert_eq!(
        headers
            .get(WEBHOOK_SIGNATURE_HEADER)
            .expect("signature header")
            .to_str()
            .expect("ascii signature"),
        webhook_signature("shared-secret", sent_at, body)
    );
}

#[tokio::test]
async fn classifies_non_success_and_timeout() {
    let (failing, _) = receiver(500, "boom").await;
    let sender = loopback_sender();
    assert_eq!(
        sender
            .deliver(
                &format!("http://127.0.0.1:{}/hook", failing.port()),
                "secret",
                UnixMillis::new(0),
                "{}",
            )
            .await,
        Err(WebhookDeliveryFailure::NonSuccessStatus)
    );

    // 応答しない宛先はtimeoutとして分類する。
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind silent listener");
    let port = silent.local_addr().expect("silent address").port();
    tokio::spawn(async move {
        let _keep_open = silent.accept().await;
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
    let quick = loopback_sender().with_timeout(Duration::from_millis(200));
    assert_eq!(
        quick
            .deliver(
                &format!("http://127.0.0.1:{port}/hook"),
                "secret",
                UnixMillis::new(0),
                "{}",
            )
            .await,
        Err(WebhookDeliveryFailure::TimedOut)
    );
}

#[tokio::test]
async fn rejects_destinations_outside_the_public_https_policy() {
    let sender = WebhookHttpSender::new(Vec::new());
    for url in [
        // allowlistがなければloopbackはaddress検査で拒否される。
        "https://127.0.0.1/hook",
        // HTTPSでないURL。
        "http://receiver.example.test/hook",
        // userinfo付きURL。
        "https://user:pass@receiver.example.test/hook",
        // 許可していないport。
        "https://receiver.example.test:8443/hook",
        // private・link-local・multicast・IPv6 loopbackのaddress literal。
        "https://10.0.0.8/hook",
        "https://169.254.0.5/hook",
        "https://224.0.0.1/hook",
        "https://[::1]/hook",
        "https://[fc00::1]/hook",
    ] {
        assert_eq!(
            sender
                .deliver(url, "secret", UnixMillis::new(0), "{}")
                .await,
            Err(WebhookDeliveryFailure::DestinationRejected),
            "{url} は拒否される必要があります"
        );
    }
}

#[tokio::test]
async fn verifies_destination_by_echoed_challenge() {
    let (echoing, _) = receiver(200, "challenge-token-123").await;
    let sender = loopback_sender();
    sender
        .verify_destination(
            &format!("http://127.0.0.1:{}/hook", echoing.port()),
            "secret",
            UnixMillis::new(0),
            "challenge-token-123",
        )
        .await
        .expect("verification succeeds");

    let (wrong, _) = receiver(200, "unrelated body").await;
    assert_eq!(
        sender
            .verify_destination(
                &format!("http://127.0.0.1:{}/hook", wrong.port()),
                "secret",
                UnixMillis::new(0),
                "challenge-token-123",
            )
            .await,
        Err(WebhookDeliveryFailure::DestinationRejected)
    );
}
