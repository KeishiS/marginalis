//! 期限切れの添付画像、認証状態、同期状態を物理削除する。

use marginalis_domain::UnixMillis;

use crate::{SqliteDatabase, SqliteStoreError, database_error};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OperationalStatePurgeCounts {
    pub web_sessions: u64,
    pub oidc_login_attempts: u64,
    pub mcp_access_tokens: u64,
    pub mcp_refresh_tokens: u64,
    pub mcp_authorization_codes: u64,
    pub mcp_client_authorizations: u64,
    pub mcp_clients: u64,
    pub note_sync_cursors: u64,
    pub note_sync_projection_entries: u64,
}

impl SqliteDatabase {
    /// 保持期限を過ぎ、どの版からも参照されない添付画像を削除する。
    pub async fn purge_unreferenced_note_attachments_before(
        &self,
        cutoff: UnixMillis,
    ) -> Result<u64, SqliteStoreError> {
        let result = sqlx::query(
            "DELETE FROM note_attachments AS attachment
             WHERE attachment.created_at_ms < ?
               AND NOT EXISTS (
                    SELECT 1 FROM note_revision_attachments AS reference
                    WHERE reference.note_id = attachment.note_id
                      AND reference.attachment_id = attachment.attachment_id
               )",
        )
        .bind(cutoff.get())
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(result.rows_affected())
    }

    /// 期限切れ・失効済みの認証状態、同期状態、参照されない変更記録とMCP clientを
    /// 一つのtransactionで削除する。
    pub async fn purge_expired_operational_state(
        &self,
        now: UnixMillis,
        unused_client_cutoff: UnixMillis,
    ) -> Result<OperationalStatePurgeCounts, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let web_sessions = sqlx::query(
            "DELETE FROM web_sessions
             WHERE revoked_at_ms IS NOT NULL
                OR idle_expires_at_ms <= ?
                OR absolute_expires_at_ms <= ?",
        )
        .bind(now.get())
        .bind(now.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?
        .rows_affected();
        let oidc_login_attempts =
            sqlx::query("DELETE FROM oidc_login_attempts WHERE expires_at_ms <= ?")
                .bind(now.get())
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?
                .rows_affected();
        let mcp_access_tokens = sqlx::query(
            "DELETE FROM mcp_access_tokens
             WHERE expires_at_ms <= ? OR revoked_at_ms IS NOT NULL",
        )
        .bind(now.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?
        .rows_affected();
        let mcp_refresh_tokens = sqlx::query(
            "DELETE FROM mcp_refresh_tokens AS stale
             WHERE stale.revoked_at_ms IS NOT NULL
                OR (
                    (stale.expires_at_ms <= ? OR stale.rotated_at_ms IS NOT NULL)
                    AND NOT EXISTS (
                        SELECT 1 FROM mcp_refresh_tokens AS active
                        WHERE active.token_family_id = stale.token_family_id
                          AND active.rotated_at_ms IS NULL
                          AND active.revoked_at_ms IS NULL
                          AND active.expires_at_ms > ?
                    )
                )",
        )
        .bind(now.get())
        .bind(now.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?
        .rows_affected();
        let mcp_authorization_codes = sqlx::query(
            "DELETE FROM mcp_authorization_codes AS stale
             WHERE stale.expires_at_ms <= ?
               AND (
                    stale.token_family_id IS NULL
                    OR (
                        NOT EXISTS (
                            SELECT 1 FROM mcp_access_tokens AS access
                            WHERE access.token_family_id = stale.token_family_id
                        )
                        AND NOT EXISTS (
                            SELECT 1 FROM mcp_refresh_tokens AS refresh
                            WHERE refresh.token_family_id = stale.token_family_id
                        )
                    )
               )",
        )
        .bind(now.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?
        .rows_affected();
        let mcp_client_authorizations = sqlx::query(
            "DELETE FROM mcp_client_authorizations AS authorizations
             WHERE COALESCE(authorizations.last_used_at_ms, authorizations.authorized_at_ms) < ?
               AND NOT EXISTS (
                   SELECT 1 FROM mcp_authorization_codes
                   WHERE principal_id = authorizations.principal_id
                     AND client_id = authorizations.client_id
               )
               AND NOT EXISTS (
                   SELECT 1 FROM mcp_access_tokens
                   WHERE principal_id = authorizations.principal_id
                     AND client_id = authorizations.client_id
               )
               AND NOT EXISTS (
                   SELECT 1 FROM mcp_refresh_tokens
                   WHERE principal_id = authorizations.principal_id
                     AND client_id = authorizations.client_id
               )",
        )
        .bind(unused_client_cutoff.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?
        .rows_affected();
        let mcp_clients = sqlx::query(
            "DELETE FROM mcp_clients
             WHERE registered_at_ms < ?
               AND NOT EXISTS (SELECT 1 FROM mcp_authorization_codes WHERE client_id = mcp_clients.client_id)
               AND NOT EXISTS (SELECT 1 FROM mcp_access_tokens WHERE client_id = mcp_clients.client_id)
               AND NOT EXISTS (SELECT 1 FROM mcp_refresh_tokens WHERE client_id = mcp_clients.client_id)
               AND NOT EXISTS (SELECT 1 FROM mcp_client_scope_ceilings WHERE client_id = mcp_clients.client_id)
               AND NOT EXISTS (SELECT 1 FROM mcp_client_authorizations WHERE client_id = mcp_clients.client_id)",
        )
        .bind(unused_client_cutoff.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?
        .rows_affected();
        let note_sync_cursors =
            sqlx::query("DELETE FROM note_sync_cursors WHERE expires_at_ms <= ?")
                // 有効期限後もしばらくhashだけを残し、期限切れと未知のcursorを区別する。
                .bind(
                    now.get()
                        .saturating_sub(marginalis_application::NOTE_SYNC_CURSOR_RETENTION_MS),
                )
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?
                .rows_affected();
        let sync_horizon = now
            .get()
            .saturating_sub(marginalis_application::NOTE_SYNC_CURSOR_RETENTION_MS);
        let note_sync_projection_entries = sqlx::query(
            "DELETE FROM note_sync_projection
             WHERE EXISTS (
                 SELECT 1 FROM domain_changes change
                 WHERE change.change_sequence = note_sync_projection.change_sequence
                   AND change.occurred_at_ms <= ?
             )",
        )
        .bind(sync_horizon)
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?
        .rows_affected();
        // 同期投影とWebhook配送のどちらからも参照されない変更記録だけを削除する。
        sqlx::query(
            "DELETE FROM domain_changes
             WHERE occurred_at_ms <= ?
               AND NOT EXISTS (
                   SELECT 1 FROM note_sync_projection sync
                   WHERE sync.change_sequence = domain_changes.change_sequence
               )
               AND NOT EXISTS (
                   SELECT 1 FROM webhook_deliveries delivery
                   WHERE delivery.event_sequence = domain_changes.change_sequence
               )",
        )
        .bind(sync_horizon)
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(OperationalStatePurgeCounts {
            web_sessions,
            oidc_login_attempts,
            mcp_access_tokens,
            mcp_refresh_tokens,
            mcp_authorization_codes,
            mcp_client_authorizations,
            mcp_clients,
            note_sync_cursors,
            note_sync_projection_entries,
        })
    }
}
