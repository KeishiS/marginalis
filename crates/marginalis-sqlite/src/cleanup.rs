//! 期限切れの認証状態を、発行経路から独立して物理削除する。

use marginalis_domain::UnixMillis;

use crate::{SqliteDatabase, SqliteStoreError, database_error};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuthStatePurgeCounts {
    pub web_sessions: u64,
    pub oidc_login_attempts: u64,
    pub mcp_access_tokens: u64,
    pub mcp_refresh_tokens: u64,
    pub mcp_authorization_codes: u64,
    pub mcp_clients: u64,
}

impl SqliteDatabase {
    /// 期限切れ・失効済み認証状態と、参照されない古いMCP clientを一transactionで削除する。
    pub async fn purge_expired_auth_state(
        &self,
        now: UnixMillis,
        unused_client_cutoff: UnixMillis,
    ) -> Result<AuthStatePurgeCounts, SqliteStoreError> {
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
        let mcp_clients = sqlx::query(
            "DELETE FROM mcp_clients
             WHERE registered_at_ms < ?
               AND NOT EXISTS (SELECT 1 FROM mcp_authorization_codes WHERE client_id = mcp_clients.client_id)
               AND NOT EXISTS (SELECT 1 FROM mcp_access_tokens WHERE client_id = mcp_clients.client_id)
               AND NOT EXISTS (SELECT 1 FROM mcp_refresh_tokens WHERE client_id = mcp_clients.client_id)",
        )
        .bind(unused_client_cutoff.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?
        .rows_affected();
        transaction.commit().await.map_err(database_error)?;
        Ok(AuthStatePurgeCounts {
            web_sessions,
            oidc_login_attempts,
            mcp_access_tokens,
            mcp_refresh_tokens,
            mcp_authorization_codes,
            mcp_clients,
        })
    }
}
