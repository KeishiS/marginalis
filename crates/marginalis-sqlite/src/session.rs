//! Web sessionと一回限りのOIDC login attemptの永続化。

use std::future::Future;

use marginalis_application::{OidcLoginAttempt, OidcLoginAttemptStore};
use marginalis_domain::{Actor, AuthenticatedSession, UnixMillis, WebSession};
use sqlx::{Row, SqlitePool};

use crate::{SqliteDatabase, SqliteStoreError, database_error, token::hash_token};

const MAX_PENDING_OIDC_LOGIN_ATTEMPTS: i64 = 1_024;

#[derive(Clone, Debug)]
pub struct SqliteOidcLoginAttemptStore {
    pool: SqlitePool,
}

impl SqliteDatabase {
    pub fn oidc_login_attempt_store(&self) -> SqliteOidcLoginAttemptStore {
        SqliteOidcLoginAttemptStore {
            pool: self.pool.clone(),
        }
    }

    /// Web sessionの不透明値はhashだけを保存する。
    pub async fn issue_web_session(
        &self,
        session: &WebSession,
        now: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        sqlx::query(
            "INSERT INTO web_sessions
             (session_id_hash, csrf_token_hash, issuer, subject,
              issued_at_ms, last_seen_at_ms, idle_expires_at_ms, absolute_expires_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(hash_token(&session.session_id))
        .bind(hash_token(&session.csrf_token))
        .bind(session.actor.issuer())
        .bind(session.actor.subject())
        .bind(now.get())
        .bind(now.get())
        .bind(session.idle_expires_at.get())
        .bind(session.absolute_expires_at.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(())
    }

    /// sessionの期限を検証し、活動中なら絶対期限を上限としてidle期限を延長する。
    pub async fn lookup_web_session(
        &self,
        session_id: &str,
        now: UnixMillis,
        idle_timeout_ms: i64,
    ) -> Result<Option<AuthenticatedSession>, SqliteStoreError> {
        let hash = hash_token(session_id);
        if idle_timeout_ms <= 0 {
            sqlx::query(
                "UPDATE web_sessions SET revoked_at_ms = ? WHERE session_id_hash = ? AND revoked_at_ms IS NULL",
            )
            .bind(now.get())
            .bind(&hash)
            .execute(&self.pool)
            .await
            .map_err(database_error)?;
            return Ok(None);
        }

        // 有効性の検証と期限延長を一つの書き込みにまとめる。読み取り後に遅延
        // transactionを更新へ切り替えると、並行要求とのsnapshot競合が即時失敗する。
        let next_idle_expires_at = now.get().saturating_add(idle_timeout_ms);
        let row = sqlx::query(
            "UPDATE web_sessions
             SET last_seen_at_ms = ?,
                 idle_expires_at_ms = MIN(absolute_expires_at_ms, ?)
             WHERE session_id_hash = ?
               AND revoked_at_ms IS NULL
               AND idle_expires_at_ms > ?
               AND absolute_expires_at_ms > ?
             RETURNING issuer, subject, idle_expires_at_ms, absolute_expires_at_ms",
        )
        .bind(now.get())
        .bind(next_idle_expires_at)
        .bind(&hash)
        .bind(now.get())
        .bind(now.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        if let Some(row) = row {
            return session_from_row(row).map(Some);
        }

        // 期限切れの行を失効済みにして、明示的なcleanup前にも再利用できない状態を残す。
        sqlx::query(
            "UPDATE web_sessions
             SET revoked_at_ms = ?
             WHERE session_id_hash = ?
               AND revoked_at_ms IS NULL
               AND (idle_expires_at_ms <= ? OR absolute_expires_at_ms <= ?)",
        )
        .bind(now.get())
        .bind(hash)
        .bind(now.get())
        .bind(now.get())
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(None)
    }

    pub async fn validate_web_session_csrf(
        &self,
        session_id: &str,
        csrf_token: &str,
    ) -> Result<bool, SqliteStoreError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM web_sessions
             WHERE session_id_hash = ? AND csrf_token_hash = ? AND revoked_at_ms IS NULL",
        )
        .bind(hash_token(session_id))
        .bind(hash_token(csrf_token))
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?
        .is_some();
        Ok(exists)
    }

    pub async fn revoke_web_session(
        &self,
        session_id: &str,
        now: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        sqlx::query(
            "UPDATE web_sessions SET revoked_at_ms = ? WHERE session_id_hash = ? AND revoked_at_ms IS NULL",
        )
        .bind(now.get())
        .bind(hash_token(session_id))
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }
}

fn session_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<AuthenticatedSession, SqliteStoreError> {
    Ok(AuthenticatedSession {
        actor: Actor::try_new(
            row.try_get("issuer").map_err(database_error)?,
            row.try_get("subject").map_err(database_error)?,
        )
        .map_err(|_| SqliteStoreError::CorruptData)?,
        idle_expires_at: UnixMillis::new(
            row.try_get("idle_expires_at_ms").map_err(database_error)?,
        ),
        absolute_expires_at: UnixMillis::new(
            row.try_get("absolute_expires_at_ms")
                .map_err(database_error)?,
        ),
    })
}

impl OidcLoginAttemptStore for SqliteOidcLoginAttemptStore {
    type Error = sqlx::Error;

    fn issue(
        &self,
        attempt: OidcLoginAttempt,
        now: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            sqlx::query("DELETE FROM oidc_login_attempts WHERE expires_at_ms <= ?")
                .bind(now.get())
                .execute(&mut *transaction)
                .await?;
            let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM oidc_login_attempts")
                .fetch_one(&mut *transaction)
                .await?;
            if count >= MAX_PENDING_OIDC_LOGIN_ATTEMPTS {
                return Err(sqlx::Error::Protocol(
                    "too many pending OIDC login attempts".into(),
                ));
            }
            sqlx::query(
                "INSERT INTO oidc_login_attempts (state_hash, nonce, pkce_verifier, expires_at_ms)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(hash_token(&attempt.state))
            .bind(attempt.nonce)
            .bind(attempt.pkce_verifier)
            .bind(attempt.expires_at.get())
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            Ok(())
        }
    }

    fn consume(
        &self,
        state: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<OidcLoginAttempt>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let hash = hash_token(&state);
            let row = sqlx::query(
                "DELETE FROM oidc_login_attempts
                 WHERE state_hash = ?
                 RETURNING nonce, pkce_verifier, expires_at_ms",
            )
            .bind(hash)
            .fetch_optional(&pool)
            .await?;
            row.map(|row| {
                let attempt = OidcLoginAttempt {
                    state,
                    nonce: row.try_get("nonce")?,
                    pkce_verifier: row.try_get("pkce_verifier")?,
                    expires_at: UnixMillis::new(row.try_get("expires_at_ms")?),
                };
                Ok((attempt.expires_at > now).then_some(attempt))
            })
            .transpose()
            .map(Option::flatten)
        }
    }
}
