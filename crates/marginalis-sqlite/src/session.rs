//! Web sessionと一回限りのOIDC login attemptの永続化。

use std::{future::Future, time::Duration};

use marginalis_application::{OidcLoginAttempt, OidcLoginAttemptStore};
use marginalis_domain::{Identity, UnixMillis};
use oidc_browser_login::session::{
    AuthenticatedWebSession, Principal, TokenDigest, WebSessionRecord, WebSessionStore,
};
use sqlx::{Row, SqlitePool};

use crate::{
    SqliteDatabase, SqliteStoreError, database_error, principal::resolve_or_create,
    token::hash_token,
};

const MAX_PENDING_OIDC_LOGIN_ATTEMPTS: i64 = 1_024;

#[derive(Clone, Debug)]
pub struct SqliteOidcLoginAttemptStore {
    pool: SqlitePool,
}

/// 共有crateの`WebSessionStore`契約に対するSQLite実装。
///
/// keyと保存値は共有crateが計算したSHA-256 digestで、平文tokenも秘密同士の比較も
/// この層には現れない。失効は行を残したまま`revoked_at_ms`を記録するsoft revoke。
#[derive(Clone, Debug)]
pub struct SqliteWebSessionStore {
    pool: SqlitePool,
}

impl SqliteDatabase {
    pub fn oidc_login_attempt_store(&self) -> SqliteOidcLoginAttemptStore {
        SqliteOidcLoginAttemptStore {
            pool: self.pool.clone(),
        }
    }

    pub fn web_session_store(&self) -> SqliteWebSessionStore {
        SqliteWebSessionStore {
            pool: self.pool.clone(),
        }
    }
}

impl WebSessionStore for SqliteWebSessionStore {
    type Error = SqliteStoreError;

    async fn issue(
        &self,
        record: WebSessionRecord,
        now: oidc_browser_login::UnixMillis,
    ) -> Result<(), Self::Error> {
        let identity = Identity::new(
            record.principal.issuer().to_owned(),
            record.principal.subject().to_owned(),
        )
        .map_err(|_| SqliteStoreError::CorruptData)?;
        let database = SqliteDatabase {
            pool: self.pool.clone(),
        };
        let (actor, identity_id) = resolve_or_create(&database, identity)
            .await
            .map_err(storage_to_store_error)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        sqlx::query(
            "INSERT INTO web_sessions
             (session_id_hash, csrf_token_hash, principal_id, authenticated_identity_id,
              issued_at_ms, last_seen_at_ms, idle_expires_at_ms, absolute_expires_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(record.session_digest.as_bytes().to_vec())
        .bind(record.csrf_digest.as_bytes().to_vec())
        .bind(actor.principal_id().get())
        .bind(identity_id)
        .bind(now.get())
        .bind(now.get())
        .bind(record.idle_expires_at.get())
        .bind(record.absolute_expires_at.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)
    }

    async fn lookup_and_extend(
        &self,
        session_digest: TokenDigest,
        now: oidc_browser_login::UnixMillis,
        idle_window: Duration,
    ) -> Result<Option<AuthenticatedWebSession>, Self::Error> {
        let digest = session_digest.as_bytes().to_vec();
        let idle_window_ms = i64::try_from(idle_window.as_millis()).unwrap_or(i64::MAX);
        // 有効性の検証と期限延長を一つの書き込みにまとめる。読み取り後に遅延
        // transactionを更新へ切り替えると、並行要求とのsnapshot競合が即時失敗する。
        let next_idle_expires_at = now.get().saturating_add(idle_window_ms);
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let row = sqlx::query(
            "UPDATE web_sessions
             SET last_seen_at_ms = ?,
                 idle_expires_at_ms = MIN(absolute_expires_at_ms, ?)
             WHERE session_id_hash = ?
               AND revoked_at_ms IS NULL
               AND idle_expires_at_ms > ?
               AND absolute_expires_at_ms > ?
             RETURNING authenticated_identity_id, idle_expires_at_ms, absolute_expires_at_ms",
        )
        .bind(now.get())
        .bind(next_idle_expires_at)
        .bind(&digest)
        .bind(now.get())
        .bind(now.get())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database_error)?;
        if let Some(row) = row {
            let identity_id = row
                .try_get("authenticated_identity_id")
                .map_err(database_error)?;
            let principal = principal_from_identity_id(&mut transaction, identity_id).await?;
            let session = session_from_row(&row, principal)?;
            transaction.commit().await.map_err(database_error)?;
            return Ok(Some(session));
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
        .bind(digest)
        .bind(now.get())
        .bind(now.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(None)
    }

    async fn csrf_digest(
        &self,
        session_digest: TokenDigest,
    ) -> Result<Option<TokenDigest>, Self::Error> {
        let stored = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT csrf_token_hash FROM web_sessions
             WHERE session_id_hash = ? AND revoked_at_ms IS NULL",
        )
        .bind(session_digest.as_bytes().to_vec())
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        stored
            .map(|bytes| {
                <[u8; 32]>::try_from(bytes.as_slice())
                    .map(TokenDigest::from_bytes)
                    .map_err(|_| SqliteStoreError::CorruptData)
            })
            .transpose()
    }

    async fn revoke(
        &self,
        session_digest: TokenDigest,
        now: oidc_browser_login::UnixMillis,
    ) -> Result<(), Self::Error> {
        sqlx::query(
            "UPDATE web_sessions SET revoked_at_ms = ? WHERE session_id_hash = ? AND revoked_at_ms IS NULL",
        )
        .bind(now.get())
        .bind(session_digest.as_bytes().to_vec())
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }
}

fn session_from_row(
    row: &sqlx::sqlite::SqliteRow,
    principal: Principal,
) -> Result<AuthenticatedWebSession, SqliteStoreError> {
    Ok(AuthenticatedWebSession {
        principal,
        idle_expires_at: oidc_browser_login::UnixMillis::new(
            row.try_get("idle_expires_at_ms").map_err(database_error)?,
        ),
        absolute_expires_at: oidc_browser_login::UnixMillis::new(
            row.try_get("absolute_expires_at_ms")
                .map_err(database_error)?,
        ),
    })
}

async fn principal_from_identity_id(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    identity_id: i64,
) -> Result<Principal, SqliteStoreError> {
    let row = sqlx::query("SELECT issuer, subject FROM principal_identities WHERE identity_id = ?")
        .bind(identity_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error)?
        .ok_or(SqliteStoreError::CorruptData)?;
    Principal::new(
        row.try_get("issuer").map_err(database_error)?,
        row.try_get("subject").map_err(database_error)?,
    )
    .map_err(|_| SqliteStoreError::CorruptData)
}

fn storage_to_store_error(error: marginalis_application::StorageError) -> SqliteStoreError {
    match error {
        marginalis_application::StorageError::CorruptData => SqliteStoreError::CorruptData,
        _ => SqliteStoreError::Database(error.to_string()),
    }
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
