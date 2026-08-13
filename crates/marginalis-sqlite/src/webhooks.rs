//! Webhook配送状態の永続化。outboxの取得、結果の記録、保持期限の削除。

use async_trait::async_trait;
use marginalis_application::{
    StorageError, WEBHOOK_RETENTION_MS, WebhookDeliveryFailure, WebhookDeliveryRepository,
    WebhookOutboxEvent, WebhookPendingDelivery,
};
use marginalis_domain::UnixMillis;
use sqlx::Row;

use crate::{SqliteDatabase, storage_error};

#[async_trait]
impl WebhookDeliveryRepository for SqliteDatabase {
    async fn claim_due_deliveries(
        &self,
        now: UnixMillis,
        lease_until: UnixMillis,
        limit: u32,
    ) -> Result<Vec<WebhookPendingDelivery>, StorageError> {
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        // subscriptionごとの先頭の未配送だけを対象に、期限とleaseを検査して占有する。
        // 先頭以外は取得しないため、同じ送信先へは順序どおりに届く。
        let rows = sqlx::query(
            "SELECT d.subscription_id, d.event_sequence
             FROM webhook_deliveries d
             JOIN webhook_subscriptions s ON s.subscription_id = d.subscription_id
             WHERE d.state = 'pending'
               AND s.state = 'active'
               AND d.next_attempt_at_ms <= ?
               AND (d.lease_expires_at_ms IS NULL OR d.lease_expires_at_ms <= ?)
               AND d.event_sequence = (
                   SELECT MIN(head.event_sequence) FROM webhook_deliveries head
                   WHERE head.subscription_id = d.subscription_id
                     AND head.state = 'pending'
               )
             ORDER BY d.event_sequence
             LIMIT ?",
        )
        .bind(now.get())
        .bind(now.get())
        .bind(i64::from(limit))
        .fetch_all(&mut *transaction)
        .await
        .map_err(storage_error)?;
        let mut claimed = Vec::with_capacity(rows.len());
        for row in rows {
            let subscription_id: String = row.get("subscription_id");
            let event_sequence: i64 = row.get("event_sequence");
            let updated = sqlx::query(
                "UPDATE webhook_deliveries SET lease_expires_at_ms = ?
                 WHERE subscription_id = ? AND event_sequence = ? AND state = 'pending'",
            )
            .bind(lease_until.get())
            .bind(&subscription_id)
            .bind(event_sequence)
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
            if updated.rows_affected() == 1 {
                claimed.push((subscription_id, event_sequence));
            }
        }
        let mut deliveries = Vec::with_capacity(claimed.len());
        for (subscription_id, event_sequence) in claimed {
            let row = sqlx::query(
                "SELECT s.url, s.secret, d.attempt_count,
                        e.event_id, e.event_kind, e.target_id, e.revision, e.occurred_at_ms
                 FROM webhook_deliveries d
                 JOIN webhook_subscriptions s ON s.subscription_id = d.subscription_id
                 JOIN webhook_outbox_events e ON e.event_sequence = d.event_sequence
                 WHERE d.subscription_id = ? AND d.event_sequence = ?",
            )
            .bind(&subscription_id)
            .bind(event_sequence)
            .fetch_one(&mut *transaction)
            .await
            .map_err(storage_error)?;
            deliveries.push(WebhookPendingDelivery {
                subscription_id,
                url: row.get("url"),
                secret: row.get("secret"),
                attempt_count: u32::try_from(row.get::<i64, _>("attempt_count")).unwrap_or(0),
                event: WebhookOutboxEvent {
                    event_id: row.get("event_id"),
                    kind: row.get("event_kind"),
                    sequence: event_sequence,
                    target_id: row.get("target_id"),
                    revision: row.get("revision"),
                    occurred_at: UnixMillis::new(row.get("occurred_at_ms")),
                },
            });
        }
        transaction.commit().await.map_err(storage_error)?;
        Ok(deliveries)
    }

    async fn record_delivered(
        &self,
        subscription_id: &str,
        event_sequence: i64,
        delivered_at: UnixMillis,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE webhook_deliveries
             SET state = 'delivered', lease_expires_at_ms = NULL,
                 last_failure = NULL, last_attempted_at_ms = ?
             WHERE subscription_id = ? AND event_sequence = ? AND state = 'pending'",
        )
        .bind(delivered_at.get())
        .bind(subscription_id)
        .bind(event_sequence)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(())
    }

    async fn record_failed(
        &self,
        subscription_id: &str,
        event_sequence: i64,
        failure: WebhookDeliveryFailure,
        attempt_count: u32,
        next_attempt_at: UnixMillis,
        attempted_at: UnixMillis,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE webhook_deliveries
             SET attempt_count = ?, next_attempt_at_ms = ?, lease_expires_at_ms = NULL,
                 last_failure = ?, last_attempted_at_ms = ?
             WHERE subscription_id = ? AND event_sequence = ? AND state = 'pending'",
        )
        .bind(i64::from(attempt_count))
        .bind(next_attempt_at.get())
        .bind(failure.as_str())
        .bind(attempted_at.get())
        .bind(subscription_id)
        .bind(event_sequence)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(())
    }

    async fn disable_exhausted_subscription(
        &self,
        subscription_id: &str,
        disabled_at: UnixMillis,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE webhook_subscriptions
             SET state = 'disabled', disabled_reason = 'delivery_exhausted',
                 updated_at_ms = ?, revision = revision + 1
             WHERE subscription_id = ? AND state = 'active'",
        )
        .bind(disabled_at.get())
        .bind(subscription_id)
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(())
    }

    async fn purge_expired_events(&self, now: UnixMillis) -> Result<u64, StorageError> {
        let horizon = now.get().saturating_sub(WEBHOOK_RETENTION_MS);
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        // 保持期間を過ぎた配送済み・破棄済みの配送行を先に消し、どこからも
        // 参照されなくなった古いeventを消す。未配送が残るeventは保持する。
        sqlx::query(
            "DELETE FROM webhook_deliveries
             WHERE state IN ('delivered', 'discarded') AND last_attempted_at_ms <= ?",
        )
        .bind(horizon)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        let events = sqlx::query(
            "DELETE FROM webhook_outbox_events
             WHERE occurred_at_ms <= ?
               AND NOT EXISTS (
                   SELECT 1 FROM webhook_deliveries d
                   WHERE d.event_sequence = webhook_outbox_events.event_sequence
               )",
        )
        .bind(horizon)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        transaction.commit().await.map_err(storage_error)?;
        Ok(events.rows_affected())
    }
}
