//! Webhook配送の状態遷移と、永続化・送信adapterとの境界。

use async_trait::async_trait;
use marginalis_domain::UnixMillis;

use crate::StorageError;

/// payloadの契約版。受信側が形の変更を検知できるように本文へ含める。
pub const WEBHOOK_CONTRACT_VERSION: u32 = 1;
/// 配送の試行上限。超えたsubscriptionは停止理由付きで無効化する。
pub const WEBHOOK_MAX_ATTEMPTS: u32 = 10;
/// 再試行間隔の初期値と上限。指数backoffで増やす。
pub const WEBHOOK_BACKOFF_BASE_MS: i64 = 5_000;
pub const WEBHOOK_BACKOFF_MAX_MS: i64 = 3_600_000;
/// 1回の取得で配送を占有する期間。期限が切れた占有は他の取得で引き継げる。
pub const WEBHOOK_LEASE_MS: i64 = 60_000;
/// 配送済みeventの保持期間。
pub const WEBHOOK_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// 1 tickで取得する配送数の上限。
pub const WEBHOOK_DELIVERY_BATCH: u32 = 4;

/// 配送するevent。本文やCSL-JSONは含まない。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookEvent {
    pub event_id: String,
    pub kind: String,
    pub sequence: i64,
    pub target_id: String,
    pub revision: i64,
    pub occurred_at: UnixMillis,
}

/// 取得した配送1件。subscriptionごとに順序の先頭だけが取得される。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookPendingDelivery {
    pub subscription_id: String,
    pub url: String,
    pub secret: String,
    /// 今回を含めない、これまでの試行回数。
    pub attempt_count: u32,
    pub event: WebhookEvent,
}

/// 配送の失敗分類。DBの`last_failure`と同じ語彙を使う。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebhookDeliveryFailure {
    NonSuccessStatus,
    ConnectFailed,
    TimedOut,
    DestinationRejected,
}

impl WebhookDeliveryFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NonSuccessStatus => "non_success_status",
            Self::ConnectFailed => "connect_failed",
            Self::TimedOut => "timed_out",
            Self::DestinationRejected => "destination_rejected",
        }
    }
}

/// 配送状態の永続化。実装はSQLite adapterが持つ。
#[async_trait]
pub trait WebhookDeliveryRepository: Send + Sync {
    /// 期限が来た配送を、subscriptionごとの先頭からlease付きで取得する。
    async fn claim_due_deliveries(
        &self,
        now: UnixMillis,
        lease_until: UnixMillis,
        limit: u32,
    ) -> Result<Vec<WebhookPendingDelivery>, StorageError>;
    async fn record_delivered(
        &self,
        subscription_id: &str,
        event_sequence: i64,
        delivered_at: UnixMillis,
    ) -> Result<(), StorageError>;
    /// 失敗を記録して次回試行を予約し、leaseを解放する。
    async fn record_failed(
        &self,
        subscription_id: &str,
        event_sequence: i64,
        failure: WebhookDeliveryFailure,
        attempt_count: u32,
        next_attempt_at: UnixMillis,
        attempted_at: UnixMillis,
    ) -> Result<(), StorageError>;
    /// 試行上限へ達したsubscriptionを無効化する。配送行は表示と再送のため残す。
    async fn disable_exhausted_subscription(
        &self,
        subscription_id: &str,
        disabled_at: UnixMillis,
    ) -> Result<(), StorageError>;
    /// 保持期間を過ぎた配送済み状態と、どの用途からも参照されない変更記録を削除する。
    async fn purge_expired_deliveries(&self, now: UnixMillis) -> Result<u64, StorageError>;
}

/// 外部URLへの送信。実装は送信直前の宛先検査と署名を行う。
#[async_trait]
pub trait WebhookDeliverySender: Send + Sync {
    /// 署名付きで本文を送る。2xx以外は失敗として分類する。
    async fn deliver(
        &self,
        url: &str,
        secret: &str,
        sent_at: UnixMillis,
        body: &str,
    ) -> Result<(), WebhookDeliveryFailure>;
    /// 送信先の所有確認。challengeを署名付きで送り、応答本文への同じ値の出現を確認する。
    async fn verify_destination(
        &self,
        url: &str,
        secret: &str,
        sent_at: UnixMillis,
        challenge: &str,
    ) -> Result<(), WebhookDeliveryFailure>;
}

/// 受信側へ送るJSON本文。契約はこの形だけを保証する。
pub fn webhook_delivery_body(event: &WebhookEvent) -> String {
    serde_json::json!({
        "contract_version": WEBHOOK_CONTRACT_VERSION,
        "event_id": event.event_id,
        "kind": event.kind,
        "sequence": event.sequence,
        "target_id": event.target_id,
        "revision": event.revision,
        "occurred_at_ms": event.occurred_at.get(),
    })
    .to_string()
}

/// 失敗後の待ち時間。試行回数に応じて指数的に増やし、上限で頭打ちにする。
pub fn webhook_backoff_ms(attempts_so_far: u32) -> i64 {
    let exponent = attempts_so_far.saturating_sub(1).min(30);
    WEBHOOK_BACKOFF_BASE_MS
        .saturating_mul(1_i64 << exponent)
        .min(WEBHOOK_BACKOFF_MAX_MS)
}

/// 1 tickの結果。ログはcomposition rootがこの値から記録する。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WebhookTickOutcome {
    pub delivered: Vec<String>,
    pub failed: Vec<(String, WebhookDeliveryFailure)>,
    pub disabled: Vec<String>,
}

/// 期限が来た配送を1回分処理する。
///
/// 取得(lease)→送信→記録の順で、1件ずつ直列に扱う。停止シグナルとの選択はループを持つ側が行う。
pub async fn webhook_delivery_tick(
    repository: &dyn WebhookDeliveryRepository,
    sender: &dyn WebhookDeliverySender,
    now: UnixMillis,
) -> Result<WebhookTickOutcome, StorageError> {
    let lease_until = UnixMillis::new(now.get().saturating_add(WEBHOOK_LEASE_MS));
    let claimed = repository
        .claim_due_deliveries(now, lease_until, WEBHOOK_DELIVERY_BATCH)
        .await?;
    let mut outcome = WebhookTickOutcome::default();
    for delivery in claimed {
        let body = webhook_delivery_body(&delivery.event);
        match sender
            .deliver(&delivery.url, &delivery.secret, now, &body)
            .await
        {
            Ok(()) => {
                repository
                    .record_delivered(&delivery.subscription_id, delivery.event.sequence, now)
                    .await?;
                outcome.delivered.push(delivery.subscription_id.clone());
            }
            Err(failure) => {
                let attempts = delivery.attempt_count.saturating_add(1);
                let next_attempt_at =
                    UnixMillis::new(now.get().saturating_add(webhook_backoff_ms(attempts)));
                repository
                    .record_failed(
                        &delivery.subscription_id,
                        delivery.event.sequence,
                        failure,
                        attempts,
                        next_attempt_at,
                        now,
                    )
                    .await?;
                outcome
                    .failed
                    .push((delivery.subscription_id.clone(), failure));
                if attempts >= WEBHOOK_MAX_ATTEMPTS {
                    repository
                        .disable_exhausted_subscription(&delivery.subscription_id, now)
                        .await?;
                    outcome.disabled.push(delivery.subscription_id.clone());
                }
            }
        }
    }
    Ok(outcome)
}
