//! Webhook配送のportと、1回分の配送を組み立てるworker。
//!
//! 配送の永続状態(outboxとlease)はrepository portが、外部URLへのHTTP送信は
//! sender portが担う。workerは両者を組み合わせて「期限の来た配送を取得し、
//! 送信し、結果を記録する」1回分(tick)だけを持ち、常駐のループと停止は
//! composition root(service)が受け持つ。

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
/// 1 tickで取得する配送数の上限(同時配送数)。
pub const WEBHOOK_DELIVERY_BATCH: u32 = 4;

/// 配送するevent。本文やCSL-JSONは含まない。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookOutboxEvent {
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
    pub event: WebhookOutboxEvent,
}

/// 配送の失敗分類。DBのlast_failureと同じ語彙を使う。
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
    /// 保持期間を過ぎた配送済みeventと、参照されなくなったeventを削除する。
    async fn purge_expired_events(&self, now: UnixMillis) -> Result<u64, StorageError>;
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
    /// 送信先の所有確認。challengeを署名付きで送り、応答本文への同じ値の
    /// 出現を確認する。
    async fn verify_destination(
        &self,
        url: &str,
        secret: &str,
        sent_at: UnixMillis,
        challenge: &str,
    ) -> Result<(), WebhookDeliveryFailure>;
}

/// 受信側へ送るJSON本文。契約はこの形だけを保証する。
pub fn webhook_delivery_body(event: &WebhookOutboxEvent) -> String {
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
/// 取得(lease)→送信→記録の順で、1件ずつ直列に扱う。取得上限が同時配送数の
/// 上限を兼ねる。停止シグナルとの選択はループを持つ側が行う。
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn delivery(attempts: u32) -> WebhookPendingDelivery {
        WebhookPendingDelivery {
            subscription_id: "sub-a".into(),
            url: "https://receiver.example.test/hook".into(),
            secret: "secret".into(),
            attempt_count: attempts,
            event: WebhookOutboxEvent {
                event_id: "event-1".into(),
                kind: "note.created".into(),
                sequence: 7,
                target_id: "note-1".into(),
                revision: 3,
                occurred_at: UnixMillis::new(1_000),
            },
        }
    }

    #[derive(Default)]
    struct StubRepository {
        due: Mutex<Vec<WebhookPendingDelivery>>,
        delivered: Mutex<Vec<i64>>,
        failed: Mutex<Vec<(u32, i64)>>,
        disabled: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl WebhookDeliveryRepository for StubRepository {
        async fn claim_due_deliveries(
            &self,
            _now: UnixMillis,
            _lease_until: UnixMillis,
            _limit: u32,
        ) -> Result<Vec<WebhookPendingDelivery>, StorageError> {
            Ok(self.due.lock().expect("due lock").drain(..).collect())
        }
        async fn record_delivered(
            &self,
            _subscription_id: &str,
            event_sequence: i64,
            _delivered_at: UnixMillis,
        ) -> Result<(), StorageError> {
            self.delivered
                .lock()
                .expect("delivered lock")
                .push(event_sequence);
            Ok(())
        }
        async fn record_failed(
            &self,
            _subscription_id: &str,
            _event_sequence: i64,
            _failure: WebhookDeliveryFailure,
            attempt_count: u32,
            next_attempt_at: UnixMillis,
            _attempted_at: UnixMillis,
        ) -> Result<(), StorageError> {
            self.failed
                .lock()
                .expect("failed lock")
                .push((attempt_count, next_attempt_at.get()));
            Ok(())
        }
        async fn disable_exhausted_subscription(
            &self,
            subscription_id: &str,
            _disabled_at: UnixMillis,
        ) -> Result<(), StorageError> {
            self.disabled
                .lock()
                .expect("disabled lock")
                .push(subscription_id.into());
            Ok(())
        }
        async fn purge_expired_events(&self, _now: UnixMillis) -> Result<u64, StorageError> {
            Ok(0)
        }
    }

    struct StubSender {
        result: Result<(), WebhookDeliveryFailure>,
        bodies: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl WebhookDeliverySender for StubSender {
        async fn deliver(
            &self,
            _url: &str,
            _secret: &str,
            _sent_at: UnixMillis,
            body: &str,
        ) -> Result<(), WebhookDeliveryFailure> {
            self.bodies.lock().expect("body lock").push(body.into());
            self.result
        }
        async fn verify_destination(
            &self,
            _url: &str,
            _secret: &str,
            _sent_at: UnixMillis,
            _challenge: &str,
        ) -> Result<(), WebhookDeliveryFailure> {
            self.result
        }
    }

    /// 成功した配送は記録され、本文は契約の項目だけを含む。
    #[tokio::test]
    async fn tick_records_success_and_sends_the_contract_payload() {
        let repository = StubRepository::default();
        repository.due.lock().expect("due lock").push(delivery(0));
        let sender = StubSender {
            result: Ok(()),
            bodies: Mutex::new(Vec::new()),
        };
        let outcome = webhook_delivery_tick(&repository, &sender, UnixMillis::new(10_000))
            .await
            .expect("tick");
        assert_eq!(outcome.delivered, vec!["sub-a".to_string()]);
        assert_eq!(
            *repository.delivered.lock().expect("delivered lock"),
            vec![7]
        );
        let bodies = sender.bodies.lock().expect("body lock");
        let payload: serde_json::Value = serde_json::from_str(&bodies[0]).expect("payload is JSON");
        assert_eq!(
            payload,
            serde_json::json!({
                "contract_version": 1,
                "event_id": "event-1",
                "kind": "note.created",
                "sequence": 7,
                "target_id": "note-1",
                "revision": 3,
                "occurred_at_ms": 1_000,
            })
        );
    }

    /// 失敗はbackoff付きで再試行を予約し、上限に達したら無効化する。
    #[tokio::test]
    async fn tick_schedules_backoff_and_disables_after_the_final_attempt() {
        let repository = StubRepository::default();
        repository.due.lock().expect("due lock").push(delivery(0));
        let sender = StubSender {
            result: Err(WebhookDeliveryFailure::TimedOut),
            bodies: Mutex::new(Vec::new()),
        };
        let outcome = webhook_delivery_tick(&repository, &sender, UnixMillis::new(10_000))
            .await
            .expect("tick");
        assert_eq!(
            outcome.failed,
            vec![("sub-a".to_string(), WebhookDeliveryFailure::TimedOut)]
        );
        assert!(outcome.disabled.is_empty());
        // 1回目の失敗は5秒後に再試行する。
        assert_eq!(
            *repository.failed.lock().expect("failed lock"),
            vec![(1, 15_000)]
        );

        // 最後の試行の失敗で無効化する。
        repository
            .due
            .lock()
            .expect("due lock")
            .push(delivery(WEBHOOK_MAX_ATTEMPTS - 1));
        let outcome = webhook_delivery_tick(&repository, &sender, UnixMillis::new(20_000))
            .await
            .expect("final tick");
        assert_eq!(outcome.disabled, vec!["sub-a".to_string()]);
        assert_eq!(
            *repository.disabled.lock().expect("disabled lock"),
            vec!["sub-a".to_string()]
        );
    }

    /// backoffは指数的に増え、上限で頭打ちになる。
    #[test]
    fn backoff_grows_exponentially_and_caps() {
        assert_eq!(webhook_backoff_ms(1), 5_000);
        assert_eq!(webhook_backoff_ms(2), 10_000);
        assert_eq!(webhook_backoff_ms(5), 80_000);
        assert_eq!(webhook_backoff_ms(10), 2_560_000);
        assert_eq!(webhook_backoff_ms(11), WEBHOOK_BACKOFF_MAX_MS);
        assert_eq!(webhook_backoff_ms(30), WEBHOOK_BACKOFF_MAX_MS);
    }
}
