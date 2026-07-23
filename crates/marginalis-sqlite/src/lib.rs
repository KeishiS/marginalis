//! MarginalisのSQLite adapterと、version管理されたschema migration。

use std::{fmt, future::Future, str::FromStr, time::Duration};

use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString},
};
use marginalis_application::{
    AuthenticatedSession, JournalEntry, NoteAclStore, NoteOperationKind, NoteProjectionStore,
    OidcIdentityStore, OidcLoginAttempt, OidcLoginAttemptStore, OidcUserAdministrationStore,
    OperationId, OperationJournal, OperationState, RootCredentialStore, WebSession,
    WebSessionStore,
};
use marginalis_domain::{
    Actor, EntityId, NoteId, NotePermission, NoteProjection, OidcIdentity, OidcLoginResult,
    OidcUser, RegistrationPolicy, SourceRevision, UnixMillis, UserId, UserStatus,
};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_initial.sql")),
    (2, include_str!("../migrations/0002_live_notes_index.sql")),
];

#[derive(Clone, Debug)]
pub struct SqliteDatabase {
    pool: SqlitePool,
}

/// 操作ジャーナルのSQLite実装。
#[derive(Clone, Debug)]
pub struct SqliteOperationJournal {
    pool: SqlitePool,
}

/// OIDC identityを内部ユーザーへ永続対応付けするSQLite adapter。
#[derive(Clone, Debug)]
pub struct SqliteOidcIdentityStore {
    pool: SqlitePool,
}

/// ノート検索投影のSQLite実装。
#[derive(Clone, Debug)]
pub struct SqliteNoteProjectionStore {
    pool: SqlitePool,
}

#[derive(Clone, Debug)]
pub struct SqliteNoteAclStore {
    pool: SqlitePool,
}

#[derive(Debug)]
pub enum NoteAclStoreError {
    Database(sqlx::Error),
    InvalidPermission,
    LastAdmin,
}
impl fmt::Display for NoteAclStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("note ACL query failed")
    }
}
impl std::error::Error for NoteAclStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<sqlx::Error> for NoteAclStoreError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Clone, Debug)]
pub struct SqliteWebSessionStore {
    pool: SqlitePool,
}

#[derive(Clone, Debug)]
pub struct SqliteOidcLoginAttemptStore {
    pool: SqlitePool,
}

#[derive(Clone, Debug)]
pub struct SqliteRootCredentialStore {
    pool: SqlitePool,
}

#[derive(Clone, Debug)]
pub struct SqliteOidcUserAdministrationStore {
    pool: SqlitePool,
}

#[derive(Debug)]
pub enum RootCredentialStoreError {
    Database(sqlx::Error),
    PasswordHash,
}
impl fmt::Display for RootCredentialStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("root credential initialization failed")
    }
}
impl std::error::Error for RootCredentialStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            Self::PasswordHash => None,
        }
    }
}
impl From<sqlx::Error> for RootCredentialStoreError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Debug)]
pub enum OidcUserAdministrationStoreError {
    Database(sqlx::Error),
    CorruptUser,
}
impl fmt::Display for OidcUserAdministrationStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OIDC user administration query failed")
    }
}
impl std::error::Error for OidcUserAdministrationStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::CorruptUser => None,
        }
    }
}
impl From<sqlx::Error> for OidcUserAdministrationStoreError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Debug)]
pub enum OidcLoginAttemptStoreError {
    Database(sqlx::Error),
}

impl fmt::Display for OidcLoginAttemptStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OIDC login attempt query failed")
    }
}

impl std::error::Error for OidcLoginAttemptStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
        }
    }
}

impl From<sqlx::Error> for OidcLoginAttemptStoreError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Debug)]
pub enum WebSessionStoreError {
    Database(sqlx::Error),
    CorruptSession,
}

impl fmt::Display for WebSessionStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("web session query failed")
    }
}
impl std::error::Error for WebSessionStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::CorruptSession => None,
        }
    }
}
impl From<sqlx::Error> for WebSessionStoreError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Debug)]
pub enum NoteProjectionError {
    Database(sqlx::Error),
}

impl fmt::Display for NoteProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("note projection query failed")
    }
}

impl std::error::Error for NoteProjectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
        }
    }
}

impl From<sqlx::Error> for NoteProjectionError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Debug)]
pub enum OidcIdentityStoreError {
    Database(sqlx::Error),
    CorruptUser,
}

impl fmt::Display for OidcIdentityStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "OIDC identity query failed: {error}"),
            Self::CorruptUser => {
                formatter.write_str("OIDC identity store contains an invalid user")
            }
        }
    }
}

impl std::error::Error for OidcIdentityStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::CorruptUser => None,
        }
    }
}

impl From<sqlx::Error> for OidcIdentityStoreError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Debug)]
pub enum JournalError {
    Database(sqlx::Error),
    CorruptEntry,
    InvalidTransition,
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "operation journal query failed: {error}"),
            Self::CorruptEntry => {
                formatter.write_str("operation journal contains an invalid entry")
            }
            Self::InvalidTransition => {
                formatter.write_str("operation journal state transition is invalid")
            }
        }
    }
}

impl std::error::Error for JournalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::CorruptEntry | Self::InvalidTransition => None,
        }
    }
}

impl From<sqlx::Error> for JournalError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

impl SqliteDatabase {
    /// 接続設定とmigrationを一箇所に集約する。
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let options = database_url
            .parse::<SqliteConnectOptions>()?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        migrate(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn operation_journal(&self) -> SqliteOperationJournal {
        SqliteOperationJournal {
            pool: self.pool.clone(),
        }
    }

    pub fn oidc_identity_store(&self) -> SqliteOidcIdentityStore {
        SqliteOidcIdentityStore {
            pool: self.pool.clone(),
        }
    }

    pub fn note_projection_store(&self) -> SqliteNoteProjectionStore {
        SqliteNoteProjectionStore {
            pool: self.pool.clone(),
        }
    }

    pub fn note_acl_store(&self) -> SqliteNoteAclStore {
        SqliteNoteAclStore {
            pool: self.pool.clone(),
        }
    }

    pub fn web_session_store(&self) -> SqliteWebSessionStore {
        SqliteWebSessionStore {
            pool: self.pool.clone(),
        }
    }

    pub fn oidc_login_attempt_store(&self) -> SqliteOidcLoginAttemptStore {
        SqliteOidcLoginAttemptStore {
            pool: self.pool.clone(),
        }
    }

    pub fn root_credential_store(&self) -> SqliteRootCredentialStore {
        SqliteRootCredentialStore {
            pool: self.pool.clone(),
        }
    }

    pub fn oidc_user_administration_store(&self) -> SqliteOidcUserAdministrationStore {
        SqliteOidcUserAdministrationStore {
            pool: self.pool.clone(),
        }
    }
}

impl NoteAclStore for SqliteNoteAclStore {
    type Error = NoteAclStoreError;
    fn permission_for(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> impl Future<Output = Result<Option<NotePermission>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            if actor.is_root {
                return Ok(Some(NotePermission::Admin));
            }
            let value =
                sqlx::query("SELECT permission FROM note_acl WHERE note_id = ? AND user_id = ?")
                    .bind(note_id.to_string())
                    .bind(actor.user_id.to_string())
                    .fetch_optional(&pool)
                    .await?
                    .map(|row| row.try_get::<i64, _>("permission"))
                    .transpose()?;
            value.map(permission_from_storage).transpose()
        }
    }
    fn set_permission(
        &self,
        note_id: NoteId,
        user_id: UserId,
        permission: Option<NotePermission>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            let current =
                sqlx::query("SELECT permission FROM note_acl WHERE note_id = ? AND user_id = ?")
                    .bind(note_id.to_string())
                    .bind(user_id.to_string())
                    .fetch_optional(&mut *transaction)
                    .await?
                    .map(|row| row.try_get::<i64, _>("permission"))
                    .transpose()?;
            if current == Some(3) && permission != Some(NotePermission::Admin) {
                let count: i64 = sqlx::query(
                    "SELECT COUNT(*) AS count FROM note_acl WHERE note_id = ? AND permission = 3",
                )
                .bind(note_id.to_string())
                .fetch_one(&mut *transaction)
                .await?
                .try_get("count")?;
                if count <= 1 {
                    return Err(NoteAclStoreError::LastAdmin);
                }
            }
            match permission {
                Some(value) => sqlx::query("INSERT INTO note_acl(note_id, user_id, permission) VALUES (?, ?, ?) ON CONFLICT(note_id, user_id) DO UPDATE SET permission = excluded.permission")
                    .bind(note_id.to_string()).bind(user_id.to_string()).bind(permission_to_storage(value)).execute(&mut *transaction).await?,
                None => sqlx::query("DELETE FROM note_acl WHERE note_id = ? AND user_id = ?")
                    .bind(note_id.to_string()).bind(user_id.to_string()).execute(&mut *transaction).await?,
            };
            transaction.commit().await?;
            Ok(())
        }
    }
}

fn permission_to_storage(value: NotePermission) -> i64 {
    match value {
        NotePermission::Read => 1,
        NotePermission::Write => 2,
        NotePermission::Admin => 3,
    }
}
fn permission_from_storage(value: i64) -> Result<NotePermission, NoteAclStoreError> {
    match value {
        1 => Ok(NotePermission::Read),
        2 => Ok(NotePermission::Write),
        3 => Ok(NotePermission::Admin),
        _ => Err(NoteAclStoreError::InvalidPermission),
    }
}

impl RootCredentialStore for SqliteRootCredentialStore {
    type Error = RootCredentialStoreError;

    fn is_initialized(&self) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            Ok(sqlx::query("SELECT 1 FROM root_credentials LIMIT 1")
                .fetch_optional(&pool)
                .await?
                .is_some())
        }
    }

    fn initialize_if_missing(
        &self,
        password: String,
        user_id: UserId,
        now: UnixMillis,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            if password.is_empty() {
                return Err(RootCredentialStoreError::PasswordHash);
            }
            let salt = SaltString::generate(&mut rand::thread_rng());
            let password_hash = Argon2::default()
                .hash_password(password.as_bytes(), &salt)
                .map_err(|_| RootCredentialStoreError::PasswordHash)?
                .to_string();
            let mut transaction = pool.begin().await?;
            if sqlx::query("SELECT 1 FROM root_credentials LIMIT 1")
                .fetch_optional(&mut *transaction)
                .await?
                .is_some()
            {
                transaction.commit().await?;
                return Ok(false);
            }
            sqlx::query("INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms) VALUES (?, 'root', 'active', 'root', ?, ?)")
                .bind(user_id.to_string()).bind(now.get()).bind(now.get()).execute(&mut *transaction).await?;
            sqlx::query("INSERT INTO root_credentials (user_id, password_hash) VALUES (?, ?)")
                .bind(user_id.to_string())
                .bind(password_hash)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            Ok(true)
        }
    }

    fn verify_password(
        &self,
        password: String,
    ) -> impl Future<Output = Result<Option<UserId>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let row = sqlx::query("SELECT user_id, password_hash FROM root_credentials LIMIT 1")
                .fetch_optional(&pool)
                .await?;
            let Some(row) = row else {
                return Ok(None);
            };
            let hash: String = row.try_get("password_hash")?;
            let parsed_hash =
                PasswordHash::new(&hash).map_err(|_| RootCredentialStoreError::PasswordHash)?;
            if Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_err()
            {
                return Ok(None);
            }
            let user_id: String = row.try_get("user_id")?;
            EntityId::from_str(&user_id)
                .map(UserId::new)
                .map(Some)
                .map_err(|_| RootCredentialStoreError::PasswordHash)
        }
    }
}

impl OidcUserAdministrationStore for SqliteOidcUserAdministrationStore {
    type Error = OidcUserAdministrationStoreError;

    fn list_pending(&self) -> impl Future<Output = Result<Vec<OidcUser>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let rows = sqlx::query(
                "SELECT user_id, status, display_name FROM users
                 WHERE authentication_kind = 'oidc' AND status = 'pending'
                 ORDER BY created_at_ms ASC, user_id ASC",
            )
            .fetch_all(&pool)
            .await?;
            rows.into_iter()
                .map(|row| {
                    oidc_user_from_row(&row)
                        .map_err(|_| OidcUserAdministrationStoreError::CorruptUser)
                })
                .collect()
        }
    }

    fn activate(
        &self,
        user_id: UserId,
        now: UnixMillis,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let result = sqlx::query(
                "UPDATE users SET status = 'active', updated_at_ms = ?
                 WHERE user_id = ? AND authentication_kind = 'oidc' AND status = 'pending'",
            )
            .bind(now.get())
            .bind(user_id.to_string())
            .execute(&pool)
            .await?;
            Ok(result.rows_affected() == 1)
        }
    }
}

impl OidcLoginAttemptStore for SqliteOidcLoginAttemptStore {
    type Error = OidcLoginAttemptStoreError;

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
            let state_hash = hash_token(&state);
            let mut transaction = pool.begin().await?;
            let row = sqlx::query(
                "DELETE FROM oidc_login_attempts
                 WHERE state_hash = ? AND expires_at_ms > ?
                 RETURNING nonce, pkce_verifier, expires_at_ms",
            )
            .bind(&state_hash)
            .bind(now.get())
            .fetch_optional(&mut *transaction)
            .await?;
            // Expired attempts are also removed, without revealing whether they existed.
            sqlx::query("DELETE FROM oidc_login_attempts WHERE state_hash = ?")
                .bind(state_hash)
                .execute(&mut *transaction)
                .await?;
            transaction.commit().await?;
            row.map(
                |row| -> Result<OidcLoginAttempt, OidcLoginAttemptStoreError> {
                    Ok(OidcLoginAttempt {
                        state,
                        nonce: row.try_get("nonce")?,
                        pkce_verifier: row.try_get("pkce_verifier")?,
                        expires_at: UnixMillis::new(row.try_get("expires_at_ms")?),
                    })
                },
            )
            .transpose()
        }
    }
}

impl WebSessionStore for SqliteWebSessionStore {
    type Error = WebSessionStoreError;
    fn issue(
        &self,
        session: WebSession,
        now: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            sqlx::query("INSERT INTO web_sessions (session_id_hash, csrf_token_hash, user_id, idle_timeout_ms, issued_at_ms, last_seen_at_ms, idle_expires_at_ms, absolute_expires_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(hash_token(&session.session_id)).bind(hash_token(&session.csrf_token))
                .bind(session.actor.user_id.to_string()).bind(session.idle_expires_at.get() - now.get())
                .bind(now.get()).bind(now.get()).bind(session.idle_expires_at.get()).bind(session.absolute_expires_at.get())
                .execute(&pool).await?;
            Ok(())
        }
    }
    fn lookup(
        &self,
        session_id: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<AuthenticatedSession>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let hash = hash_token(&session_id);
            let row = sqlx::query("SELECT web_sessions.user_id, users.authentication_kind, idle_timeout_ms, idle_expires_at_ms, absolute_expires_at_ms FROM web_sessions JOIN users ON users.user_id = web_sessions.user_id WHERE session_id_hash = ? AND revoked_at_ms IS NULL")
                .bind(&hash).fetch_optional(&pool).await?;
            let Some(row) = row else { return Ok(None) };
            let idle: i64 = row.try_get("idle_expires_at_ms")?;
            let absolute: i64 = row.try_get("absolute_expires_at_ms")?;
            if now.get() >= idle || now.get() >= absolute {
                return Ok(None);
            }
            let timeout: i64 = row.try_get("idle_timeout_ms")?;
            let next_idle = (now.get() + timeout).min(absolute);
            sqlx::query("UPDATE web_sessions SET last_seen_at_ms = ?, idle_expires_at_ms = ? WHERE session_id_hash = ?")
                .bind(now.get()).bind(next_idle).bind(hash).execute(&pool).await?;
            let user_id: String = row.try_get("user_id")?;
            let authentication_kind: String = row.try_get("authentication_kind")?;
            Ok(Some(AuthenticatedSession {
                actor: Actor {
                    user_id: UserId::new(
                        EntityId::from_str(&user_id)
                            .map_err(|_| WebSessionStoreError::CorruptSession)?,
                    ),
                    is_root: authentication_kind == "root",
                },
                idle_expires_at: UnixMillis::new(next_idle),
                absolute_expires_at: UnixMillis::new(absolute),
            }))
        }
    }
    fn revoke(
        &self,
        session_id: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            sqlx::query("UPDATE web_sessions SET revoked_at_ms = ? WHERE session_id_hash = ? AND revoked_at_ms IS NULL").bind(now.get()).bind(hash_token(&session_id)).execute(&pool).await?;
            Ok(())
        }
    }

    fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let valid = sqlx::query(
                "SELECT 1 FROM web_sessions WHERE session_id_hash = ? AND csrf_token_hash = ?
                 AND revoked_at_ms IS NULL AND idle_expires_at_ms > ? AND absolute_expires_at_ms > ?",
            )
            .bind(hash_token(&session_id))
            .bind(hash_token(&csrf_token))
            .bind(now.get())
            .bind(now.get())
            .fetch_optional(&pool)
            .await?
            .is_some();
            Ok(valid)
        }
    }
}

fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

impl NoteProjectionStore for SqliteNoteProjectionStore {
    type Error = NoteProjectionError;

    fn replace_projection(
        &self,
        projection: NoteProjection,
        revision: SourceRevision,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            sqlx::query(
                "INSERT INTO notes (note_id, relative_path, title, source_revision, deleted_at_ms)
                 VALUES (?, ?, ?, ?, NULL)
                 ON CONFLICT(note_id) DO UPDATE SET
                   title = excluded.title, source_revision = excluded.source_revision, deleted_at_ms = NULL",
            )
            .bind(projection.note_id.to_string())
            .bind(format!("notes/{}.adoc", projection.note_id))
            .bind(&projection.title)
            .bind(revision.bytes().to_vec())
            .execute(&mut *transaction).await?;
            sqlx::query(
                "INSERT INTO note_acl (note_id, user_id, permission) VALUES (?, ?, 3)
                 ON CONFLICT(note_id, user_id) DO NOTHING",
            )
            .bind(projection.note_id.to_string())
            .bind(projection.owner_id.to_string())
            .execute(&mut *transaction)
            .await?;
            sqlx::query("DELETE FROM note_anchors WHERE note_id = ?")
                .bind(projection.note_id.to_string())
                .execute(&mut *transaction)
                .await?;
            sqlx::query("DELETE FROM note_references WHERE source_note_id = ?")
                .bind(projection.note_id.to_string())
                .execute(&mut *transaction)
                .await?;
            for anchor in projection.anchors {
                sqlx::query("INSERT INTO note_anchors (note_id, anchor_id) VALUES (?, ?)")
                    .bind(projection.note_id.to_string())
                    .bind(anchor)
                    .execute(&mut *transaction)
                    .await?;
            }
            for reference in projection.references {
                sqlx::query(
                    "INSERT INTO note_references
                     (source_note_id, source_start, source_end, target_note_id, target_anchor)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(projection.note_id.to_string())
                .bind(i64::from(reference.source_start))
                .bind(i64::from(reference.source_end))
                .bind(reference.target_note_id)
                .bind(reference.target_anchor)
                .execute(&mut *transaction)
                .await?;
            }
            transaction.commit().await?;
            Ok(())
        }
    }

    fn delete_projection(
        &self,
        note_id: NoteId,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            sqlx::query("DELETE FROM notes WHERE note_id = ?")
                .bind(note_id.to_string())
                .execute(&pool)
                .await?;
            Ok(())
        }
    }
}

impl OidcIdentityStore for SqliteOidcIdentityStore {
    type Error = OidcIdentityStoreError;

    fn register_or_lookup(
        &self,
        identity: OidcIdentity,
        policy: RegistrationPolicy,
        new_user_id: UserId,
        now: UnixMillis,
    ) -> impl Future<Output = Result<OidcLoginResult, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            let existing = sqlx::query(
                "SELECT users.user_id, users.status, users.display_name
                 FROM oidc_identities JOIN users ON users.user_id = oidc_identities.user_id
                 WHERE oidc_identities.issuer = ? AND oidc_identities.subject = ?",
            )
            .bind(&identity.issuer)
            .bind(&identity.subject)
            .fetch_optional(&mut *transaction)
            .await?;
            let user = if let Some(row) = existing {
                let user = oidc_user_from_row(&row)?;
                sqlx::query(
                    "UPDATE users SET display_name = ?, updated_at_ms = ? WHERE user_id = ?",
                )
                .bind(&identity.display_name)
                .bind(now.get())
                .bind(user.user_id.to_string())
                .execute(&mut *transaction)
                .await?;
                OidcUser {
                    display_name: identity.display_name,
                    ..user
                }
            } else {
                if policy == RegistrationPolicy::InviteOnly {
                    transaction.commit().await?;
                    return Ok(OidcLoginResult::RegistrationDenied);
                }
                let status = match policy {
                    RegistrationPolicy::Open => UserStatus::Active,
                    RegistrationPolicy::Approval => UserStatus::Pending,
                    RegistrationPolicy::InviteOnly => unreachable!("handled above"),
                };
                let user = OidcUser {
                    user_id: new_user_id,
                    status,
                    display_name: identity.display_name,
                };
                sqlx::query(
                    "INSERT INTO users
                     (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
                     VALUES (?, 'oidc', ?, ?, ?, ?)",
                )
                .bind(user.user_id.to_string())
                .bind(status.as_storage())
                .bind(&user.display_name)
                .bind(now.get())
                .bind(now.get())
                .execute(&mut *transaction)
                .await?;
                sqlx::query(
                    "INSERT INTO oidc_identities (issuer, subject, user_id) VALUES (?, ?, ?)",
                )
                .bind(&identity.issuer)
                .bind(&identity.subject)
                .bind(user.user_id.to_string())
                .execute(&mut *transaction)
                .await?;
                user
            };
            transaction.commit().await?;
            Ok(match user.status {
                UserStatus::Active => OidcLoginResult::Active(user),
                UserStatus::Pending => OidcLoginResult::PendingApproval(user),
                UserStatus::Disabled => OidcLoginResult::Disabled(user),
            })
        }
    }
}

impl OperationJournal for SqliteOperationJournal {
    type Error = JournalError;

    fn prepare(&self, entry: JournalEntry) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            if entry.state != OperationState::Prepared {
                return Err(JournalError::InvalidTransition);
            }
            sqlx::query(
                "INSERT INTO operation_journal
                 (operation_id, kind, state, note_id, source_revision, projection_payload, created_at_ms, updated_at_ms)
                 VALUES (?, ?, 'prepared', ?, ?, ?, ?, ?)",
            )
            .bind(entry.operation_id.0.to_string())
            .bind(operation_kind(entry.kind))
            .bind(entry.note_id.to_string())
            .bind(
                entry
                    .source_revision
                    .map(|revision| revision.bytes().to_vec()),
            )
            .bind(entry.projection.map(|projection| serde_json::to_string(&projection)).transpose().map_err(|_| JournalError::CorruptEntry)?)
            .bind(entry.created_at.get())
            .bind(entry.updated_at.get())
            .execute(&pool)
            .await?;
            Ok(())
        }
    }

    fn mark_source_applied(
        &self,
        operation_id: OperationId,
        updated_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let result = sqlx::query(
                "UPDATE operation_journal SET state = 'source_applied', updated_at_ms = ?
                 WHERE operation_id = ? AND state = 'prepared'",
            )
            .bind(updated_at.get())
            .bind(operation_id.0.to_string())
            .execute(&pool)
            .await?;
            if result.rows_affected() == 1 {
                Ok(())
            } else {
                Err(JournalError::InvalidTransition)
            }
        }
    }

    fn complete(
        &self,
        operation_id: OperationId,
        updated_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let result = sqlx::query(
                "UPDATE operation_journal SET state = 'completed', updated_at_ms = ?
                 WHERE operation_id = ? AND state = 'source_applied'",
            )
            .bind(updated_at.get())
            .bind(operation_id.0.to_string())
            .execute(&pool)
            .await?;
            if result.rows_affected() == 1 {
                Ok(())
            } else {
                Err(JournalError::InvalidTransition)
            }
        }
    }

    fn incomplete(&self) -> impl Future<Output = Result<Vec<JournalEntry>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let rows = sqlx::query(
                "SELECT operation_id, kind, state, note_id, source_revision, projection_payload, created_at_ms, updated_at_ms
                 FROM operation_journal WHERE state <> 'completed' ORDER BY created_at_ms",
            )
            .fetch_all(&pool)
            .await?;
            rows.iter().map(entry_from_row).collect()
        }
    }
}

fn operation_kind(kind: NoteOperationKind) -> &'static str {
    match kind {
        NoteOperationKind::Create => "create",
        NoteOperationKind::Update => "update",
        NoteOperationKind::Delete => "delete",
    }
}

fn oidc_user_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<OidcUser, OidcIdentityStoreError> {
    let user_id = row.try_get::<String, _>("user_id")?;
    let status = row.try_get::<String, _>("status")?;
    Ok(OidcUser {
        user_id: UserId::new(
            EntityId::from_str(&user_id).map_err(|_| OidcIdentityStoreError::CorruptUser)?,
        ),
        status: UserStatus::from_storage(&status).ok_or(OidcIdentityStoreError::CorruptUser)?,
        display_name: row.try_get("display_name")?,
    })
}

fn entry_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<JournalEntry, JournalError> {
    let operation_id = row.try_get::<String, _>("operation_id")?;
    let note_id = row.try_get::<String, _>("note_id")?;
    let kind = match row.try_get::<String, _>("kind")?.as_str() {
        "create" => NoteOperationKind::Create,
        "update" => NoteOperationKind::Update,
        "delete" => NoteOperationKind::Delete,
        _ => return Err(JournalError::CorruptEntry),
    };
    let state = match row.try_get::<String, _>("state")?.as_str() {
        "prepared" => OperationState::Prepared,
        "source_applied" => OperationState::SourceApplied,
        _ => return Err(JournalError::CorruptEntry),
    };
    let source_revision = row
        .try_get::<Option<Vec<u8>>, _>("source_revision")?
        .map(|value| SourceRevision::from_bytes(&value).ok_or(JournalError::CorruptEntry))
        .transpose()?;
    let projection = row
        .try_get::<Option<String>, _>("projection_payload")?
        .map(|payload| serde_json::from_str(&payload).map_err(|_| JournalError::CorruptEntry))
        .transpose()?;
    Ok(JournalEntry {
        operation_id: OperationId(
            EntityId::from_str(&operation_id).map_err(|_| JournalError::CorruptEntry)?,
        ),
        note_id: NoteId::new(EntityId::from_str(&note_id).map_err(|_| JournalError::CorruptEntry)?),
        kind,
        state,
        source_revision,
        projection,
        created_at: UnixMillis::new(row.try_get("created_at_ms")?),
        updated_at: UnixMillis::new(row.try_get("updated_at_ms")?),
    })
}

/// schema versionはSQLite内で追跡する。migrationファイルは追加専用であり、既存versionを
/// 書き換えない。開発DBの破棄ではなく、upgrade testで各versionからの更新を検証する。
async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _marginalis_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            applied_at_ms INTEGER NOT NULL
        ) STRICT",
    )
    .execute(pool)
    .await?;
    for (version, sql) in MIGRATIONS {
        let applied = sqlx::query("SELECT 1 FROM _marginalis_migrations WHERE version = ?")
            .bind(version)
            .fetch_optional(pool)
            .await?
            .is_some();
        if applied {
            continue;
        }
        let mut transaction = pool.begin().await?;
        sqlx::raw_sql(sql).execute(&mut *transaction).await?;
        sqlx::query(
            "INSERT INTO _marginalis_migrations (version, applied_at_ms)
             VALUES (?, CAST(unixepoch('subsec') * 1000 AS INTEGER))",
        )
        .bind(version)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_application::{
        JournalEntry, NoteAclService, NoteAclServiceError, NoteAclStore, NoteOperationKind,
        NoteProjectionStore, OidcIdentityStore, OidcLoginAttempt, OidcLoginAttemptStore,
        OidcUserAdministrationStore, OperationId, OperationJournal, OperationState,
        RootCredentialStore,
    };
    use marginalis_domain::{
        Actor, EntityId, NoteId, NotePermission, NoteProjection, NoteReference, OidcIdentity,
        OidcLoginResult, OidcUser, RegistrationPolicy, SourceRevision, UnixMillis, UserId,
        UserStatus,
    };
    use sqlx::Row;

    use super::*;

    #[tokio::test]
    async fn applies_the_versioned_initial_schema() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("migration succeeds");
        let row = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'operation_journal'",
        )
        .fetch_one(database.pool())
        .await
        .expect("journal table exists");
        assert_eq!(row.get::<String, _>("name"), "operation_journal");
    }

    #[tokio::test]
    async fn upgrades_a_database_at_the_previous_schema_version() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("database");
        sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
            .execute(&pool)
            .await
            .expect("initial schema");
        sqlx::query(
            "CREATE TABLE _marginalis_migrations (version INTEGER PRIMARY KEY NOT NULL, applied_at_ms INTEGER NOT NULL) STRICT",
        ).execute(&pool).await.expect("migration table");
        sqlx::query("INSERT INTO _marginalis_migrations (version, applied_at_ms) VALUES (1, 0)")
            .execute(&pool)
            .await
            .expect("initial version");
        migrate(&pool).await.expect("upgrade");
        let version: i64 =
            sqlx::query("SELECT MAX(version) AS version FROM _marginalis_migrations")
                .fetch_one(&pool)
                .await
                .expect("versions")
                .try_get("version")
                .expect("version");
        assert_eq!(version, 2);
        let index: String = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name = 'notes_live_title_idx'",
        )
        .fetch_one(&pool)
        .await
        .expect("index")
        .try_get("name")
        .expect("name");
        assert_eq!(index, "notes_live_title_idx");
    }

    #[tokio::test]
    async fn journal_records_and_transitions_a_note_update() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("migration succeeds");
        let journal = database.operation_journal();
        let operation = OperationId(
            EntityId::from_str("01800000-0000-7000-8000-000000000001")
                .expect("UUIDv7 operation ID"),
        );
        let note = NoteId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000002").expect("UUIDv7 note ID"),
        );
        journal
            .prepare(JournalEntry {
                operation_id: operation,
                note_id: note,
                kind: NoteOperationKind::Update,
                state: OperationState::Prepared,
                source_revision: None,
                projection: None,
                created_at: UnixMillis::new(1),
                updated_at: UnixMillis::new(1),
            })
            .await
            .expect("record preparation");
        assert_eq!(journal.incomplete().await.expect("read journal").len(), 1);
        journal
            .mark_source_applied(operation, UnixMillis::new(2))
            .await
            .expect("mark source written");
        journal
            .complete(operation, UnixMillis::new(3))
            .await
            .expect("complete journal");
        assert!(journal.incomplete().await.expect("read journal").is_empty());
    }

    #[tokio::test]
    async fn oidc_identity_is_stable_while_display_name_is_refreshed() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("migration succeeds");
        let store = database.oidc_identity_store();
        let user_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000003").expect("UUIDv7 user ID"),
        );
        let first = store
            .register_or_lookup(
                OidcIdentity::new("https://id.example.test", "subject", "First name")
                    .expect("valid identity"),
                RegistrationPolicy::Open,
                user_id,
                UnixMillis::new(1),
            )
            .await
            .expect("register identity");
        let OidcLoginResult::Active(first) = first else {
            panic!("open registration activates user");
        };
        let second = store
            .register_or_lookup(
                OidcIdentity::new("https://id.example.test", "subject", "Renamed")
                    .expect("valid identity"),
                RegistrationPolicy::Approval,
                UserId::new(
                    EntityId::from_str("01800000-0000-7000-8000-000000000004")
                        .expect("UUIDv7 unused ID"),
                ),
                UnixMillis::new(2),
            )
            .await
            .expect("look up identity");
        let OidcLoginResult::Active(second) = second else {
            panic!("existing active user remains active");
        };
        assert_eq!(second.user_id, first.user_id);
        assert_eq!(second.display_name, "Renamed");
    }

    #[tokio::test]
    async fn projection_replaces_anchors_and_positioned_references() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("migration succeeds");
        let note_id = NoteId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000010").expect("UUIDv7 note ID"),
        );
        let owner_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000011").expect("UUIDv7 user ID"),
        );
        // The owner is normally created by OIDC/root initialization before note creation.
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'Owner', 0, 0)",
        )
        .bind(owner_id.to_string())
        .execute(database.pool())
        .await
        .expect("insert owner");
        database
            .note_projection_store()
            .replace_projection(
                NoteProjection {
                    note_id,
                    owner_id,
                    title: "Projection".into(),
                    anchors: vec!["section".into()],
                    references: vec![NoteReference {
                        source_start: 3,
                        source_end: 12,
                        target_note_id: "01800000-0000-7000-8000-000000000012".into(),
                        target_anchor: Some("target".into()),
                    }],
                },
                SourceRevision::from_source(b"= Projection\n"),
            )
            .await
            .expect("store projection");
        let count: i64 =
            sqlx::query("SELECT COUNT(*) AS count FROM note_references WHERE source_note_id = ?")
                .bind(note_id.to_string())
                .fetch_one(database.pool())
                .await
                .expect("read references")
                .try_get("count")
                .expect("count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn oidc_login_state_is_hashed_expiring_and_single_use() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let store = database.oidc_login_attempt_store();
        store
            .issue(OidcLoginAttempt {
                state: "state-secret".into(),
                nonce: "nonce".into(),
                pkce_verifier: "verifier".into(),
                expires_at: UnixMillis::new(20),
            })
            .await
            .expect("issue");
        let stored: Vec<u8> = sqlx::query("SELECT state_hash FROM oidc_login_attempts")
            .fetch_one(database.pool())
            .await
            .expect("stored attempt")
            .try_get("state_hash")
            .expect("hash");
        assert_ne!(stored, b"state-secret");
        let attempt = store
            .consume("state-secret".into(), UnixMillis::new(10))
            .await
            .expect("consume")
            .expect("attempt");
        assert_eq!(attempt.nonce, "nonce");
        assert!(
            store
                .consume("state-secret".into(), UnixMillis::new(10))
                .await
                .expect("consume again")
                .is_none()
        );
    }

    #[tokio::test]
    async fn root_credential_is_initialized_once_without_storing_plaintext() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let store = database.root_credential_store();
        assert!(!store.is_initialized().await.expect("initial state"));
        let user_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000020").expect("UUIDv7"),
        );
        assert!(
            store
                .initialize_if_missing("not-a-hash".into(), user_id, UnixMillis::new(1))
                .await
                .expect("initialize")
        );
        assert!(store.is_initialized().await.expect("initialized"));
        assert!(
            !store
                .initialize_if_missing(
                    "other-password".into(),
                    UserId::new(
                        EntityId::from_str("01800000-0000-7000-8000-000000000021").expect("UUIDv7"),
                    ),
                    UnixMillis::new(2),
                )
                .await
                .expect("second initialization")
        );
        let hash: String = sqlx::query("SELECT password_hash FROM root_credentials")
            .fetch_one(database.pool())
            .await
            .expect("credential")
            .try_get("password_hash")
            .expect("hash");
        assert_ne!(hash, "not-a-hash");
        assert_eq!(
            store
                .verify_password("not-a-hash".into())
                .await
                .expect("verify password"),
            Some(user_id)
        );
        assert!(
            store
                .verify_password("wrong-password".into())
                .await
                .expect("reject wrong password")
                .is_none()
        );
    }

    #[tokio::test]
    async fn root_can_list_and_activate_pending_oidc_users() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let user_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000022").expect("UUIDv7"),
        );
        let identities = database.oidc_identity_store();
        let result = identities
            .register_or_lookup(
                OidcIdentity::new("https://id.example.test", "pending-subject", "Pending user")
                    .expect("identity"),
                RegistrationPolicy::Approval,
                user_id,
                UnixMillis::new(1),
            )
            .await
            .expect("register pending user");
        assert!(matches!(result, OidcLoginResult::PendingApproval(_)));

        let administration = database.oidc_user_administration_store();
        assert_eq!(
            administration
                .list_pending()
                .await
                .expect("list pending users"),
            vec![OidcUser {
                user_id,
                status: UserStatus::Pending,
                display_name: "Pending user".into(),
            }]
        );
        assert!(
            administration
                .activate(user_id, UnixMillis::new(2))
                .await
                .expect("activate user")
        );
        assert!(
            administration
                .list_pending()
                .await
                .expect("list active users")
                .is_empty()
        );
        let result = identities
            .register_or_lookup(
                OidcIdentity::new("https://id.example.test", "pending-subject", "Pending user")
                    .expect("identity"),
                RegistrationPolicy::Approval,
                UserId::new(
                    EntityId::from_str("01800000-0000-7000-8000-000000000023").expect("UUIDv7"),
                ),
                UnixMillis::new(3),
            )
            .await
            .expect("look up active user");
        assert!(matches!(result, OidcLoginResult::Active(_)));
    }

    #[tokio::test]
    async fn acl_keeps_the_last_administrator_and_bypasses_for_root() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let note_id = NoteId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000030").expect("UUIDv7"),
        );
        let owner_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000031").expect("UUIDv7"),
        );
        let other_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000032").expect("UUIDv7"),
        );
        sqlx::query("INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms) VALUES (?, 'oidc', 'active', 'User', 0, 0)")
            .bind(owner_id.to_string()).execute(database.pool()).await.expect("owner");
        sqlx::query("INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms) VALUES (?, 'oidc', 'active', 'User', 0, 0)")
            .bind(other_id.to_string()).execute(database.pool()).await.expect("other user");
        database
            .note_projection_store()
            .replace_projection(
                NoteProjection {
                    note_id,
                    owner_id,
                    title: "ACL".into(),
                    anchors: Vec::new(),
                    references: Vec::new(),
                },
                SourceRevision::from_source(b"= ACL\n"),
            )
            .await
            .expect("note");
        let acl = database.note_acl_store();
        assert_eq!(
            acl.permission_for(
                Actor {
                    user_id: owner_id,
                    is_root: false
                },
                note_id
            )
            .await
            .expect("permission"),
            Some(NotePermission::Admin)
        );
        assert!(matches!(
            acl.set_permission(note_id, owner_id, None).await,
            Err(NoteAclStoreError::LastAdmin)
        ));
        assert!(matches!(
            NoteAclService::new(&acl)
                .set_permission(
                    Actor {
                        user_id: other_id,
                        is_root: false
                    },
                    note_id,
                    other_id,
                    Some(NotePermission::Read),
                )
                .await,
            Err(NoteAclServiceError::Forbidden)
        ));
        assert_eq!(
            acl.permission_for(
                Actor {
                    user_id: owner_id,
                    is_root: true
                },
                note_id
            )
            .await
            .expect("root"),
            Some(NotePermission::Admin)
        );
    }
}
