//! 期限切れの認証状態を、発行経路から独立して物理削除する。

use marginalis_domain::UnixMillis;

use crate::{SqliteDatabase, SqliteStoreError, database_error};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuthStatePurgeCounts {
    pub web_sessions: u64,
    pub oidc_login_attempts: u64,
}

impl SqliteDatabase {
    /// 期限切れ・失効済みのWeb認証状態を一transactionで削除する。
    pub async fn purge_expired_auth_state(
        &self,
        now: UnixMillis,
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
        transaction.commit().await.map_err(database_error)?;
        Ok(AuthStatePurgeCounts {
            web_sessions,
            oidc_login_attempts,
        })
    }
}
