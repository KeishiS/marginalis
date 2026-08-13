//! Webhook配送状態の永続化。outboxの取得、結果の記録、保持期限の削除。

use async_trait::async_trait;
use marginalis_application::{
    StorageError, WEBHOOK_RETENTION_MS, WebhookDeliveryFailure, WebhookDeliveryRepository,
    WebhookOutboxEvent, WebhookPendingDelivery, WebhookSubscriptionOverview,
    WebhookSubscriptionRepository, WebhookSubscriptionState,
};
use marginalis_domain::{Actor, UnixMillis};
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

#[async_trait]
impl WebhookSubscriptionRepository for SqliteDatabase {
    async fn list_owned_subscriptions(
        &self,
        actor: &Actor,
    ) -> Result<Vec<WebhookSubscriptionOverview>, StorageError> {
        // 概要と配送状況を1回で読む。配送状況は先頭の未配送行と集計から得る。
        let rows = sqlx::query(
            "SELECT s.subscription_id, s.url, s.event_kinds_json, s.state, s.disabled_reason,
                    s.created_at_ms, s.updated_at_ms, s.revision,
                    (SELECT MAX(d.last_attempted_at_ms) FROM webhook_deliveries d
                     WHERE d.subscription_id = s.subscription_id) AS last_attempted_at_ms,
                    (SELECT d.last_failure FROM webhook_deliveries d
                     WHERE d.subscription_id = s.subscription_id AND d.state = 'pending'
                     ORDER BY d.event_sequence LIMIT 1) AS last_failure,
                    (SELECT MIN(d.next_attempt_at_ms) FROM webhook_deliveries d
                     WHERE d.subscription_id = s.subscription_id AND d.state = 'pending')
                        AS next_attempt_at_ms,
                    (SELECT COUNT(*) FROM webhook_deliveries d
                     WHERE d.subscription_id = s.subscription_id AND d.state = 'pending')
                        AS pending_count
             FROM webhook_subscriptions s
             WHERE s.owner_issuer = ? AND s.owner_subject = ?
             ORDER BY s.created_at_ms, s.subscription_id",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        let mut subscriptions = Vec::with_capacity(rows.len());
        for row in rows {
            let event_kinds: Vec<String> =
                serde_json::from_str(row.get::<String, _>("event_kinds_json").as_str())
                    .map_err(|_| StorageError::CorruptData)?;
            let state = WebhookSubscriptionState::parse(row.get::<String, _>("state").as_str())
                .ok_or(StorageError::CorruptData)?;
            subscriptions.push(WebhookSubscriptionOverview {
                subscription_id: row.get("subscription_id"),
                url: row.get("url"),
                event_kinds,
                state,
                disabled_reason: row.get("disabled_reason"),
                created_at: UnixMillis::new(row.get("created_at_ms")),
                updated_at: UnixMillis::new(row.get("updated_at_ms")),
                revision: row.get("revision"),
                last_attempted_at: row
                    .get::<Option<i64>, _>("last_attempted_at_ms")
                    .map(UnixMillis::new),
                last_failure: row.get("last_failure"),
                next_attempt_at: row
                    .get::<Option<i64>, _>("next_attempt_at_ms")
                    .map(UnixMillis::new),
                pending_count: row.get("pending_count"),
            });
        }
        Ok(subscriptions)
    }

    async fn create_owned_subscription(
        &self,
        actor: &Actor,
        subscription_id: &str,
        url: &str,
        event_kinds: &[String],
        secret: &str,
        now: UnixMillis,
    ) -> Result<(), StorageError> {
        let event_kinds_json =
            serde_json::to_string(event_kinds).map_err(|_| StorageError::CorruptData)?;
        sqlx::query(
            "INSERT INTO webhook_subscriptions (
                 subscription_id, owner_issuer, owner_subject, url, secret, event_kinds_json,
                 state, created_at_ms, updated_at_ms, revision
             ) VALUES (?, ?, ?, ?, ?, ?, 'pending_challenge', ?, ?, 1)",
        )
        .bind(subscription_id)
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(url)
        .bind(secret)
        .bind(event_kinds_json)
        .bind(now.get())
        .bind(now.get())
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(())
    }

    async fn owned_subscription_credentials(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<Option<(String, String)>, StorageError> {
        let row = sqlx::query(
            "SELECT url, secret FROM webhook_subscriptions
             WHERE subscription_id = ? AND owner_issuer = ? AND owner_subject = ?",
        )
        .bind(subscription_id)
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(row.map(|row| (row.get("url"), row.get("secret"))))
    }

    async fn activate_owned_subscription(
        &self,
        actor: &Actor,
        subscription_id: &str,
        now: UnixMillis,
    ) -> Result<bool, StorageError> {
        let updated = sqlx::query(
            "UPDATE webhook_subscriptions
             SET state = 'active', disabled_reason = NULL, updated_at_ms = ?,
                 revision = revision + 1
             WHERE subscription_id = ? AND owner_issuer = ? AND owner_subject = ?",
        )
        .bind(now.get())
        .bind(subscription_id)
        .bind(actor.issuer())
        .bind(actor.subject())
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(updated.rows_affected() == 1)
    }

    async fn delete_owned_subscription(
        &self,
        actor: &Actor,
        subscription_id: &str,
    ) -> Result<bool, StorageError> {
        let deleted = sqlx::query(
            "DELETE FROM webhook_subscriptions
             WHERE subscription_id = ? AND owner_issuer = ? AND owner_subject = ?",
        )
        .bind(subscription_id)
        .bind(actor.issuer())
        .bind(actor.subject())
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(deleted.rows_affected() == 1)
    }

    async fn replace_owned_secret(
        &self,
        actor: &Actor,
        subscription_id: &str,
        secret: &str,
        now: UnixMillis,
    ) -> Result<bool, StorageError> {
        let updated = sqlx::query(
            "UPDATE webhook_subscriptions
             SET secret = ?, updated_at_ms = ?, revision = revision + 1
             WHERE subscription_id = ? AND owner_issuer = ? AND owner_subject = ?",
        )
        .bind(secret)
        .bind(now.get())
        .bind(subscription_id)
        .bind(actor.issuer())
        .bind(actor.subject())
        .execute(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(updated.rows_affected() == 1)
    }

    async fn retry_owned_head_delivery(
        &self,
        actor: &Actor,
        subscription_id: &str,
        now: UnixMillis,
    ) -> Result<bool, StorageError> {
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        let owned = sqlx::query(
            "SELECT 1 FROM webhook_subscriptions
             WHERE subscription_id = ? AND owner_issuer = ? AND owner_subject = ?",
        )
        .bind(subscription_id)
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(storage_error)?;
        if owned.is_none() {
            return Ok(false);
        }
        // 先頭の未配送を試行回数ごと初期化し、即時再試行できるようにする。
        sqlx::query(
            "UPDATE webhook_deliveries
             SET attempt_count = 0, next_attempt_at_ms = ?, lease_expires_at_ms = NULL
             WHERE subscription_id = ? AND state = 'pending'
               AND event_sequence = (
                   SELECT MIN(head.event_sequence) FROM webhook_deliveries head
                   WHERE head.subscription_id = webhook_deliveries.subscription_id
                     AND head.state = 'pending'
               )",
        )
        .bind(now.get())
        .bind(subscription_id)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        // 試行上限で無効化されていた場合は有効へ戻す。他の理由の無効化は保つ。
        sqlx::query(
            "UPDATE webhook_subscriptions
             SET state = 'active', disabled_reason = NULL, updated_at_ms = ?,
                 revision = revision + 1
             WHERE subscription_id = ? AND state = 'disabled'
               AND disabled_reason = 'delivery_exhausted'",
        )
        .bind(now.get())
        .bind(subscription_id)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        transaction.commit().await.map_err(storage_error)?;
        Ok(true)
    }

    async fn discard_owned_head_delivery(
        &self,
        actor: &Actor,
        subscription_id: &str,
        now: UnixMillis,
    ) -> Result<bool, StorageError> {
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        let owned = sqlx::query(
            "SELECT 1 FROM webhook_subscriptions
             WHERE subscription_id = ? AND owner_issuer = ? AND owner_subject = ?",
        )
        .bind(subscription_id)
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(storage_error)?;
        if owned.is_none() {
            return Ok(false);
        }
        // 先頭の未配送を破棄し、後続の配送を進められるようにする。
        sqlx::query(
            "UPDATE webhook_deliveries
             SET state = 'discarded', lease_expires_at_ms = NULL, last_attempted_at_ms = ?
             WHERE subscription_id = ? AND state = 'pending'
               AND event_sequence = (
                   SELECT MIN(head.event_sequence) FROM webhook_deliveries head
                   WHERE head.subscription_id = webhook_deliveries.subscription_id
                     AND head.state = 'pending'
               )",
        )
        .bind(now.get())
        .bind(subscription_id)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        // 破棄後も試行上限の無効化が残ると後続が止まるため、有効へ戻す。
        sqlx::query(
            "UPDATE webhook_subscriptions
             SET state = 'active', disabled_reason = NULL, updated_at_ms = ?,
                 revision = revision + 1
             WHERE subscription_id = ? AND state = 'disabled'
               AND disabled_reason = 'delivery_exhausted'",
        )
        .bind(now.get())
        .bind(subscription_id)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        transaction.commit().await.map_err(storage_error)?;
        Ok(true)
    }
}
