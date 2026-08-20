//! Webhookの送信adapter。
//!
//! 送信直前に宛先の全addressを解決して特別用途addressを拒否し、解決済みの
//! addressへ接続を固定する。redirectは追わない。本文と時刻をHMAC-SHA256で
//! 署名したheaderを付け、応答は状態codeだけで判定して本文は上限まで読み捨てる。
//! 製品内で利用するのはcomposition rootだけなので、独立crateにはしない。

use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use marginalis_application::{
    WebhookDeliveryFailure, WebhookDeliverySender, is_public_webhook_address,
    validate_webhook_destination,
};
use marginalis_domain::UnixMillis;
use sha2::Sha256;

/// 送信timeout。公開契約の値。
const WEBHOOK_SEND_TIMEOUT_MS: u64 = 10_000;
/// 応答bodyを読む上限。受信側の巨大な応答で送信workerを塞がない。
const WEBHOOK_RESPONSE_BODY_LIMIT: usize = 64 * 1024;

/// 署名headerの名前。受信側はこの2つで検証する。
const WEBHOOK_TIMESTAMP_HEADER: &str = "x-marginalis-timestamp";
const WEBHOOK_SIGNATURE_HEADER: &str = "x-marginalis-signature";

pub(crate) struct WebhookHttpSender {
    /// 管理者が明示した例外host。private networkへの配送を許可し、
    /// 検証環境ではhttpとloopbackも許す。
    allowed_hosts: Vec<String>,
    timeout: Duration,
}

impl WebhookHttpSender {
    pub(crate) fn new(allowed_hosts: Vec<String>) -> Self {
        Self {
            allowed_hosts,
            timeout: Duration::from_millis(WEBHOOK_SEND_TIMEOUT_MS),
        }
    }

    /// 送信timeoutを変更する。試験と将来の運用設定で使う。
    #[cfg(test)]
    fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// URLを検査し、接続を固定する解決済みaddressを返す。
    ///
    /// 構文とliteralの検査は登録時と同じapplication層の検査を使い、
    /// 名前で指定された宛先は送信直前にすべての解決結果を検査する。
    async fn checked_destination(
        &self,
        url: &str,
    ) -> Result<(url::Url, String, Vec<SocketAddr>), WebhookDeliveryFailure> {
        let destination = validate_webhook_destination(url, &self.allowed_hosts)
            .map_err(|_| WebhookDeliveryFailure::DestinationRejected)?;
        // IPのliteralは検査済みなのでDNSを介さず接続し、名前は解決結果を検査する。
        let addresses: Vec<SocketAddr> = match destination.literal {
            Some(address) => vec![SocketAddr::new(address, destination.port)],
            None => {
                let resolved: Vec<SocketAddr> =
                    tokio::net::lookup_host((destination.host.as_str(), destination.port))
                        .await
                        .map_err(|_| WebhookDeliveryFailure::ConnectFailed)?
                        .collect();
                if resolved.is_empty() {
                    return Err(WebhookDeliveryFailure::ConnectFailed);
                }
                resolved
            }
        };
        if !destination.exempt
            && addresses
                .iter()
                .any(|address| !is_public_webhook_address(address.ip()))
        {
            return Err(WebhookDeliveryFailure::DestinationRejected);
        }
        let parsed =
            url::Url::parse(url).map_err(|_| WebhookDeliveryFailure::DestinationRejected)?;
        Ok((parsed, destination.host, addresses))
    }

    async fn post_signed(
        &self,
        url: &str,
        secret: &str,
        sent_at: UnixMillis,
        body: String,
    ) -> Result<reqwest::Response, WebhookDeliveryFailure> {
        let (parsed, host, addresses) = self.checked_destination(url).await?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(self.timeout)
            .resolve_to_addrs(&host, &addresses)
            .build()
            .map_err(|_| WebhookDeliveryFailure::ConnectFailed)?;
        let signature = webhook_signature(secret, sent_at, &body);
        client
            .post(parsed)
            .header("content-type", "application/json")
            .header(WEBHOOK_TIMESTAMP_HEADER, sent_at.get().to_string())
            .header(WEBHOOK_SIGNATURE_HEADER, signature)
            .body(body)
            .send()
            .await
            .map_err(classify_send_error)
    }
}

/// 署名の値。`v1=`に続けて、`<時刻>.<本文>`へのHMAC-SHA256を小文字hexで並べる。
fn webhook_signature(secret: &str, sent_at: UnixMillis, body: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of every size");
    mac.update(sent_at.get().to_string().as_bytes());
    mac.update(b".");
    mac.update(body.as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut value = String::with_capacity(3 + digest.len() * 2);
    value.push_str("v1=");
    for byte in digest {
        value.push_str(&format!("{byte:02x}"));
    }
    value
}

fn classify_send_error(error: reqwest::Error) -> WebhookDeliveryFailure {
    if error.is_timeout() {
        WebhookDeliveryFailure::TimedOut
    } else {
        WebhookDeliveryFailure::ConnectFailed
    }
}

/// 応答bodyを上限まで読み、上限の範囲を文字列として返す。
async fn read_limited_body(mut response: reqwest::Response) -> String {
    let mut collected: Vec<u8> = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        let remaining = WEBHOOK_RESPONSE_BODY_LIMIT.saturating_sub(collected.len());
        if remaining == 0 {
            break;
        }
        collected.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    String::from_utf8_lossy(&collected).into_owned()
}

#[async_trait]
impl WebhookDeliverySender for WebhookHttpSender {
    async fn deliver(
        &self,
        url: &str,
        secret: &str,
        sent_at: UnixMillis,
        body: &str,
    ) -> Result<(), WebhookDeliveryFailure> {
        let response = self
            .post_signed(url, secret, sent_at, body.to_string())
            .await?;
        let status = response.status();
        // 成功でも失敗でも、応答bodyは上限まで読み捨てて接続を返す。
        let _ = read_limited_body(response).await;
        if status.is_success() {
            Ok(())
        } else {
            Err(WebhookDeliveryFailure::NonSuccessStatus)
        }
    }

    async fn verify_destination(
        &self,
        url: &str,
        secret: &str,
        sent_at: UnixMillis,
        challenge: &str,
    ) -> Result<(), WebhookDeliveryFailure> {
        let body = format!(
            "{{\"contract_version\":1,\"challenge\":\"{}\"}}",
            challenge.replace('"', "")
        );
        let response = self.post_signed(url, secret, sent_at, body).await?;
        let status = response.status();
        let body = read_limited_body(response).await;
        if status.is_success() && body.contains(challenge) {
            Ok(())
        } else {
            Err(WebhookDeliveryFailure::DestinationRejected)
        }
    }
}

#[cfg(test)]
mod tests;
