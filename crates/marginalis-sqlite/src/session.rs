//! Web sessionと一回限りのOIDC login attemptの永続化。

use std::future::Future;

use marginalis_application::{OidcLoginAttempt, OidcLoginAttemptStore};
use marginalis_domain::{Actor, AuthenticatedSession, UnixMillis, WebSession};
use sqlx::{Row, SqlitePool};

use crate::{SqliteDatabase, SqliteStoreError, database_error, token::hash_token};

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
        sqlx::query(
            "INSERT INTO web_sessions
             (session_id_hash, csrf_token_hash, issuer, subject, is_administrator,
              issued_at_ms, last_seen_at_ms, idle_expires_at_ms, absolute_expires_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(hash_token(&session.session_id))
        .bind(hash_token(&session.csrf_token))
        .bind(&session.actor.issuer)
        .bind(&session.actor.subject)
        .bind(session.actor.is_administrator)
        .bind(now.get())
        .bind(now.get())
        .bind(session.idle_expires_at.get())
        .bind(session.absolute_expires_at.get())
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    /// sessionの期限を検証し、活動中なら利用時刻だけを更新する。
    pub async fn lookup_web_session(
        &self,
        session_id: &str,
        now: UnixMillis,
    ) -> Result<Option<AuthenticatedSession>, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let hash = hash_token(session_id);
        let row = sqlx::query(
            "SELECT issuer, subject, is_administrator, idle_expires_at_ms, absolute_expires_at_ms
             FROM web_sessions WHERE session_id_hash = ? AND revoked_at_ms IS NULL",
        )
        .bind(&hash)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let session = session_from_row(row)?;
        if session.idle_expires_at <= now || session.absolute_expires_at <= now {
            sqlx::query(
                "UPDATE web_sessions SET revoked_at_ms = ? WHERE session_id_hash = ? AND revoked_at_ms IS NULL",
            )
            .bind(now.get())
            .bind(hash)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
            transaction.commit().await.map_err(database_error)?;
            return Ok(None);
        }
        sqlx::query("UPDATE web_sessions SET last_seen_at_ms = ? WHERE session_id_hash = ?")
            .bind(now.get())
            .bind(hash)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(Some(session))
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
        actor: Actor {
            issuer: row.try_get("issuer").map_err(database_error)?,
            subject: row.try_get("subject").map_err(database_error)?,
            is_administrator: row
                .try_get::<bool, _>("is_administrator")
                .map_err(database_error)?,
        },
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
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO oidc_login_attempts (state_hash, nonce, pkce_verifier, expires_at_ms)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(hash_token(&attempt.state))
            .bind(attempt.nonce)
            .bind(attempt.pkce_verifier)
            .bind(attempt.expires_at.get())
            .execute(&pool)
            .await?;
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
                 WHERE state_hash = ? AND expires_at_ms > ?
                 RETURNING nonce, pkce_verifier, expires_at_ms",
            )
            .bind(&hash)
            .bind(now.get())
            .fetch_optional(&pool)
            .await?;
            sqlx::query("DELETE FROM oidc_login_attempts WHERE state_hash = ?")
                .bind(hash)
                .execute(&pool)
                .await?;
            row.map(|row| {
                Ok(OidcLoginAttempt {
                    state,
                    nonce: row.try_get("nonce")?,
                    pkce_verifier: row.try_get("pkce_verifier")?,
                    expires_at: UnixMillis::new(row.try_get("expires_at_ms")?),
                })
            })
            .transpose()
        }
    }
}
