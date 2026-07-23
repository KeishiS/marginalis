//! MarginalisのSQLite adapterと、version管理されたschema migration。

use std::{collections::HashSet, fmt, future::Future, path::Path, str::FromStr, time::Duration};

use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString},
};
use marginalis_application::{
    AuthenticatedSession, DeleteConfirmation, DeleteConfirmationStore, JournalEntry,
    McpAccessTokenStore, McpOAuthStore, McpRefreshTokenRotation, NoteAclStore, NoteOperationKind,
    NoteProjectionStore, NoteQueryStore, OidcIdentityStore, OidcLoginAttempt,
    OidcLoginAttemptStore, OidcUserAdministrationStore, OperationId, OperationJournal,
    OperationState, RootCredentialStore, WebSession, WebSessionStore,
};
use marginalis_domain::{
    Actor, EntityId, McpAuthorizationGrant, McpClientAuthorization, McpOAuthClient, NoteId,
    NoteLink, NoteLinkPage, NotePage, NotePermission, NoteProjection, NoteSummary, OidcIdentity,
    OidcLoginResult, OidcUser, RegistrationPolicy, RootAuditEvent, SourceRevision, UnixMillis,
    UserId, UserStatus,
};
use sha2::{Digest, Sha256};
use sqlx::{
    QueryBuilder, Row, Sqlite, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_initial.sql")),
    (2, include_str!("../migrations/0002_live_notes_index.sql")),
    (3, include_str!("../migrations/0003_note_search.sql")),
    (4, include_str!("../migrations/0004_mcp_access_tokens.sql")),
    (5, include_str!("../migrations/0005_mcp_oauth.sql")),
    (
        6,
        include_str!("../migrations/0006_delete_confirmations.sql"),
    ),
    (
        7,
        include_str!("../migrations/0007_mcp_token_timestamps.sql"),
    ),
    (
        8,
        include_str!("../migrations/0008_registration_policy.sql"),
    ),
    (9, include_str!("../migrations/0009_root_audit_log.sql")),
    (
        10,
        include_str!("../migrations/0010_delete_confirmation_note_cascade.sql"),
    ),
    (
        11,
        include_str!("../migrations/0011_delete_confirmation_reference_snapshot.sql"),
    ),
    (
        12,
        include_str!("../migrations/0012_note_metadata_filters.sql"),
    ),
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
pub struct SqliteNoteQueryStore {
    pool: SqlitePool,
}

#[derive(Clone, Debug)]
pub struct SqliteNoteAclStore {
    pool: SqlitePool,
}

#[derive(Clone, Debug)]
pub struct SqliteMcpAccessTokenStore {
    pool: SqlitePool,
}

#[derive(Clone, Debug)]
pub struct SqliteMcpOAuthStore {
    pool: SqlitePool,
}

#[derive(Clone, Debug)]
pub struct SqliteDeleteConfirmationStore {
    pool: SqlitePool,
}

#[derive(Debug)]
pub enum McpOAuthStoreError {
    Database(sqlx::Error),
    Serialization(serde_json::Error),
    CorruptUser,
}

impl fmt::Display for McpOAuthStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MCP OAuth query failed")
    }
}

impl std::error::Error for McpOAuthStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Serialization(error) => Some(error),
            Self::CorruptUser => None,
        }
    }
}

impl From<sqlx::Error> for McpOAuthStoreError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

impl From<serde_json::Error> for McpOAuthStoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

#[derive(Debug)]
pub enum McpAccessTokenStoreError {
    Database(sqlx::Error),
    CorruptUser,
}

impl fmt::Display for McpAccessTokenStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MCP access token query failed")
    }
}

impl std::error::Error for McpAccessTokenStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::CorruptUser => None,
        }
    }
}

impl From<sqlx::Error> for McpAccessTokenStoreError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
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

#[derive(Debug)]
pub enum NoteQueryStoreError {
    Database(sqlx::Error),
    CorruptNote,
}
impl fmt::Display for NoteQueryStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("note query failed")
    }
}
impl std::error::Error for NoteQueryStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::CorruptNote => None,
        }
    }
}
impl From<sqlx::Error> for NoteQueryStoreError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
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
        Self::connect_with_initial_registration_policy(database_url, RegistrationPolicy::Approval)
            .await
    }

    /// 初回作成時だけ登録policyを設定して接続する。
    ///
    /// migration済みDBでは現在値を一切上書きしない。これによりNixOSの初期設定とrootによる運用時変更を
    /// 区別する。
    pub async fn connect_with_initial_registration_policy(
        database_url: &str,
        initial_registration_policy: RegistrationPolicy,
    ) -> Result<Self, sqlx::Error> {
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
        let is_new_database = sqlx::query(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_marginalis_migrations'",
        )
        .fetch_optional(&pool)
        .await?
        .is_none();
        migrate(&pool).await?;
        let database = Self { pool };
        if is_new_database {
            database
                .set_registration_policy(initial_registration_policy)
                .await?;
        }
        Ok(database)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// WALをcheckpointしてから、SQLite自身のbackup機能で一貫したdatabase fileを書き出す。
    ///
    /// `VACUUM INTO`はtransaction内で実行できないため、呼出し側はHTTP serviceを停止して、
    /// 正本filesystemとの組を取得する必要がある。
    pub async fn backup_to(&self, destination: &str) -> Result<(), sqlx::Error> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await?;
        let destination = destination.replace('\'', "''");
        sqlx::query(&format!("VACUUM INTO '{destination}'"))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// migrationや書込みを行わずに、backup SQLite fileの整合性を確認する。
    pub async fn validate_backup_file(path: &Path) -> Result<(), sqlx::Error> {
        let path = path.to_str().ok_or_else(|| {
            sqlx::Error::Protocol("database backup path is not valid UTF-8".into())
        })?;
        let options = format!("sqlite:{path}")
            .parse::<SqliteConnectOptions>()?
            .read_only(true)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let checks = sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_all(&pool)
            .await?;
        if checks.len() == 1 && checks[0] == "ok" {
            Ok(())
        } else {
            Err(sqlx::Error::Protocol(
                "SQLite integrity_check failed".into(),
            ))
        }
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

    pub fn note_query_store(&self) -> SqliteNoteQueryStore {
        SqliteNoteQueryStore {
            pool: self.pool.clone(),
        }
    }

    pub fn note_acl_store(&self) -> SqliteNoteAclStore {
        SqliteNoteAclStore {
            pool: self.pool.clone(),
        }
    }

    pub fn mcp_access_token_store(&self) -> SqliteMcpAccessTokenStore {
        SqliteMcpAccessTokenStore {
            pool: self.pool.clone(),
        }
    }

    pub fn mcp_oauth_store(&self) -> SqliteMcpOAuthStore {
        SqliteMcpOAuthStore {
            pool: self.pool.clone(),
        }
    }

    pub fn delete_confirmation_store(&self) -> SqliteDeleteConfirmationStore {
        SqliteDeleteConfirmationStore {
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

    pub async fn registration_policy(&self) -> Result<RegistrationPolicy, sqlx::Error> {
        let value: String =
            sqlx::query_scalar("SELECT policy FROM registration_policy WHERE singleton = 1")
                .fetch_one(&self.pool)
                .await?;
        Ok(match value.as_str() {
            "open" => RegistrationPolicy::Open,
            "approval" => RegistrationPolicy::Approval,
            _ => RegistrationPolicy::InviteOnly,
        })
    }
    pub async fn set_registration_policy(
        &self,
        policy: RegistrationPolicy,
    ) -> Result<(), sqlx::Error> {
        let value = match policy {
            RegistrationPolicy::Open => "open",
            RegistrationPolicy::Approval => "approval",
            RegistrationPolicy::InviteOnly => "invite-only",
        };
        sqlx::query("UPDATE registration_policy SET policy = ? WHERE singleton = 1")
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// root監査を保存する。呼出元は秘密値を渡してはならない。
    pub async fn record_root_audit(&self, event: RootAuditEvent) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO root_audit_log
             (action, actor_user_id, target_user_id, target, occurred_at_ms)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(event.action.as_storage())
        .bind(event.actor_user_id.map(|id| id.to_string()))
        .bind(event.target_user_id.map(|id| id.to_string()))
        .bind(event.target)
        .bind(event.occurred_at.get())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 保持期限より古いroot監査を消去する。呼出時刻はUTC epoch millisecondsで与える。
    pub async fn purge_root_audit_before(&self, cutoff: UnixMillis) -> Result<u64, sqlx::Error> {
        Ok(
            sqlx::query("DELETE FROM root_audit_log WHERE occurred_at_ms < ?")
                .bind(cutoff.get())
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }

    /// 期限切れまたは消費済みで再利用不能な認証補助データを削除する。
    ///
    /// root監査やノート正本・投影は対象にしない。呼出側は日次maintenanceとして実行する。
    pub async fn purge_expired_ephemera(&self, now: UnixMillis) -> Result<u64, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let mut purged = 0;
        for statement in [
            "DELETE FROM oidc_login_attempts WHERE expires_at_ms <= ?",
            "DELETE FROM delete_confirmations WHERE expires_at_ms <= ? OR consumed_at_ms IS NOT NULL",
            "DELETE FROM mcp_authorization_codes WHERE expires_at_ms <= ? OR consumed_at_ms IS NOT NULL",
            "DELETE FROM mcp_access_tokens WHERE expires_at_ms <= ? OR revoked_at_ms IS NOT NULL",
            "DELETE FROM mcp_refresh_tokens WHERE expires_at_ms <= ? OR rotated_at_ms IS NOT NULL OR revoked_at_ms IS NOT NULL",
            "DELETE FROM web_sessions WHERE idle_expires_at_ms <= ? OR absolute_expires_at_ms <= ? OR revoked_at_ms IS NOT NULL",
        ] {
            let mut query = sqlx::query(statement).bind(now.get());
            if statement.contains("absolute_expires_at_ms") {
                query = query.bind(now.get());
            }
            purged += query.execute(&mut *transaction).await?.rows_affected();
        }
        transaction.commit().await?;
        Ok(purged)
    }

    /// 全正本の検証済みprojectionでSQLite検索・参照投影を置換する。
    ///
    /// 呼出側は、すべての正本を解析し、ファイル名と文書内`note-id`の一致を確認してから呼ぶ。
    /// transaction内で古い正本由来の行を消すため、途中失敗で最後の成功したprojectionは失われない。
    pub async fn replace_all_note_projections(
        &self,
        projections: &[(NoteProjection, SourceRevision)],
    ) -> Result<(), sqlx::Error> {
        let expected_ids = projections
            .iter()
            .map(|(projection, _)| projection.note_id.to_string())
            .collect::<HashSet<_>>();
        let mut transaction = self.pool.begin().await?;
        let existing_ids = sqlx::query_scalar::<_, String>("SELECT note_id FROM notes")
            .fetch_all(&mut *transaction)
            .await?;
        for note_id in existing_ids
            .into_iter()
            .filter(|note_id| !expected_ids.contains(note_id))
        {
            sqlx::query("DELETE FROM notes WHERE note_id = ?")
                .bind(note_id)
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query("DELETE FROM note_search")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM note_anchors")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM note_references")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM note_tags")
            .execute(&mut *transaction)
            .await?;
        for (projection, revision) in projections {
            insert_note_projection_rows(&mut transaction, projection, *revision).await?;
        }
        transaction.commit().await?;
        Ok(())
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
            let value = sqlx::query(
                "SELECT note_acl.permission
                 FROM note_acl
                 JOIN users ON users.user_id = note_acl.user_id
                 WHERE note_acl.note_id = ? AND note_acl.user_id = ? AND users.status = 'active'",
            )
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
                    "SELECT COUNT(*) AS count
                     FROM note_acl
                     JOIN users ON users.user_id = note_acl.user_id
                     WHERE note_acl.note_id = ?
                       AND note_acl.permission = 3
                       AND users.status = 'active'",
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

    fn disable(
        &self,
        user_id: UserId,
        now: UnixMillis,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            let leaves_note_without_active_admin: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                   SELECT 1
                   FROM note_acl AS candidate
                   WHERE candidate.user_id = ?
                     AND candidate.permission = 3
                     AND NOT EXISTS (
                       SELECT 1
                       FROM note_acl AS other_acl
                       JOIN users AS other_user ON other_user.user_id = other_acl.user_id
                       WHERE other_acl.note_id = candidate.note_id
                         AND other_acl.permission = 3
                         AND other_acl.user_id <> candidate.user_id
                         AND other_user.status = 'active'
                     )
                 )",
            )
            .bind(user_id.to_string())
            .fetch_one(&mut *transaction)
            .await?;
            if leaves_note_without_active_admin {
                transaction.rollback().await?;
                return Ok(false);
            }
            let updated = sqlx::query(
                "UPDATE users SET status = 'disabled', updated_at_ms = ?
                 WHERE user_id = ? AND authentication_kind = 'oidc' AND status = 'active'",
            )
            .bind(now.get())
            .bind(user_id.to_string())
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if updated != 1 {
                transaction.rollback().await?;
                return Ok(false);
            }
            sqlx::query("UPDATE web_sessions SET revoked_at_ms = ? WHERE user_id = ? AND revoked_at_ms IS NULL")
                .bind(now.get()).bind(user_id.to_string()).execute(&mut *transaction).await?;
            sqlx::query("UPDATE mcp_access_tokens SET revoked_at_ms = ? WHERE user_id = ? AND revoked_at_ms IS NULL")
                .bind(now.get()).bind(user_id.to_string()).execute(&mut *transaction).await?;
            sqlx::query("UPDATE mcp_refresh_tokens SET revoked_at_ms = ? WHERE user_id = ? AND revoked_at_ms IS NULL")
                .bind(now.get()).bind(user_id.to_string()).execute(&mut *transaction).await?;
            transaction.commit().await?;
            Ok(true)
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

/// 削除対象へ向くxref集合を、利用者へ開示しない完全な状態hashと可視件数で表す。
///
/// hashには不可視な参照も含めるので、参照状態の変化を見落とさない。一方で応答へ返す
/// 件数はsource noteを閲覧できる参照だけに限定し、不可視ノートの存在を漏らさない。
async fn incoming_reference_snapshot(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    actor: Actor,
    note_id: NoteId,
) -> Result<(Vec<u8>, u64), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT source_note_id, source_start, source_end, target_anchor
         FROM note_references
         WHERE target_note_id = ?
         ORDER BY source_note_id ASC, source_start ASC, source_end ASC, target_anchor ASC",
    )
    .bind(note_id.to_string())
    .fetch_all(&mut **transaction)
    .await?;
    let mut hasher = Sha256::new();
    for row in rows {
        let source_note_id: String = row.try_get("source_note_id")?;
        let source_start: i64 = row.try_get("source_start")?;
        let source_end: i64 = row.try_get("source_end")?;
        let target_anchor: String = row.try_get("target_anchor")?;
        for value in [
            source_note_id,
            source_start.to_string(),
            source_end.to_string(),
            target_anchor,
        ] {
            hasher.update((value.len() as u64).to_be_bytes());
            hasher.update(value.as_bytes());
        }
    }
    let visible_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM note_references AS refs
         JOIN notes AS source ON source.note_id = refs.source_note_id
         WHERE refs.target_note_id = ?
           AND (? OR EXISTS (
             SELECT 1 FROM note_acl
             WHERE note_acl.note_id = source.note_id AND note_acl.user_id = ?
           ))",
    )
    .bind(note_id.to_string())
    .bind(actor.is_root)
    .bind(actor.user_id.to_string())
    .fetch_one(&mut **transaction)
    .await?;
    Ok((hasher.finalize().to_vec(), visible_count.max(0) as u64))
}

impl McpAccessTokenStore for SqliteMcpAccessTokenStore {
    type Error = McpAccessTokenStoreError;

    fn authenticate(
        &self,
        token: String,
        resource_uri: String,
        required_scope: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<Actor>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let token_hash = hash_token(&token);
            let row = sqlx::query(
                "SELECT mcp_access_tokens.user_id
                 FROM mcp_access_tokens JOIN users ON users.user_id = mcp_access_tokens.user_id
                 WHERE mcp_access_tokens.token_hash = ?
                   AND mcp_access_tokens.resource_uri = ?
                   AND mcp_access_tokens.revoked_at_ms IS NULL
                   AND mcp_access_tokens.expires_at_ms > ?
                   AND instr(' ' || mcp_access_tokens.scopes || ' ', ' ' || ? || ' ') > 0
                   AND users.status = 'active'
                   AND users.authentication_kind <> 'root'",
            )
            .bind(&token_hash)
            .bind(resource_uri)
            .bind(now.get())
            .bind(required_scope)
            .fetch_optional(&pool)
            .await?;
            let Some(row) = row else {
                return Ok(None);
            };
            sqlx::query("UPDATE mcp_access_tokens SET last_used_at_ms = ? WHERE token_hash = ?")
                .bind(now.get())
                .bind(token_hash)
                .execute(&pool)
                .await?;
            let user_id: String = row.try_get("user_id")?;
            let entity_id =
                EntityId::from_str(&user_id).map_err(|_| McpAccessTokenStoreError::CorruptUser)?;
            Ok(Some(Actor {
                user_id: UserId::new(entity_id),
                is_root: false,
            }))
        }
    }
}

impl DeleteConfirmationStore for SqliteDeleteConfirmationStore {
    type Error = McpOAuthStoreError;

    fn issue(
        &self,
        token: String,
        actor: Actor,
        note_id: NoteId,
        expected_revision: SourceRevision,
        expires_at: UnixMillis,
    ) -> impl Future<Output = Result<u64, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            let (incoming_reference_state_hash, incoming_reference_count) =
                incoming_reference_snapshot(&mut transaction, actor, note_id).await?;
            sqlx::query(
                "INSERT INTO delete_confirmations
                 (token_hash, user_id, note_id, source_revision, incoming_reference_state_hash, expires_at_ms)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(hash_token(&token))
            .bind(actor.user_id.to_string())
            .bind(note_id.to_string())
            .bind(expected_revision.to_hex())
            .bind(incoming_reference_state_hash)
            .bind(expires_at.get())
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            Ok(incoming_reference_count)
        }
    }

    fn consume(
        &self,
        token: String,
        actor: Actor,
        now: UnixMillis,
    ) -> impl Future<Output = Result<DeleteConfirmation, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            let row = sqlx::query(
                "SELECT note_id, source_revision, incoming_reference_state_hash FROM delete_confirmations
                 WHERE token_hash = ? AND user_id = ? AND consumed_at_ms IS NULL AND expires_at_ms > ?",
            )
            .bind(hash_token(&token))
            .bind(actor.user_id.to_string())
            .bind(now.get())
            .fetch_optional(&mut *transaction)
            .await?;
            let Some(row) = row else {
                transaction.rollback().await?;
                return Ok(DeleteConfirmation::Missing);
            };
            let note_id = EntityId::from_str(&row.try_get::<String, _>("note_id")?)
                .map(NoteId::new)
                .map_err(|_| McpOAuthStoreError::CorruptUser)?;
            let stored_reference_state_hash: Vec<u8> =
                row.try_get("incoming_reference_state_hash")?;
            let (current_reference_state_hash, _) =
                incoming_reference_snapshot(&mut transaction, actor, note_id).await?;
            if stored_reference_state_hash != current_reference_state_hash {
                transaction.rollback().await?;
                return Ok(DeleteConfirmation::Stale);
            }
            let updated = sqlx::query(
                "UPDATE delete_confirmations SET consumed_at_ms = ?
                 WHERE token_hash = ? AND consumed_at_ms IS NULL",
            )
            .bind(now.get())
            .bind(hash_token(&token))
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if updated != 1 {
                transaction.rollback().await?;
                return Ok(DeleteConfirmation::Missing);
            }
            let revision = SourceRevision::from_hex(&row.try_get::<String, _>("source_revision")?)
                .ok_or(McpOAuthStoreError::CorruptUser)?;
            transaction.commit().await?;
            Ok(DeleteConfirmation::Confirmed {
                note_id,
                expected_revision: revision,
            })
        }
    }
}

impl McpOAuthStore for SqliteMcpOAuthStore {
    type Error = McpOAuthStoreError;

    fn upsert_client(
        &self,
        client: McpOAuthClient,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let redirect_uris = serde_json::to_string(&client.redirect_uris)?;
            sqlx::query(
                "INSERT INTO mcp_oauth_clients (client_id, display_name, redirect_uris_json)
                 VALUES (?, ?, ?)
                 ON CONFLICT(client_id) DO UPDATE SET
                   display_name = excluded.display_name,
                   redirect_uris_json = excluded.redirect_uris_json",
            )
            .bind(client.client_id)
            .bind(client.display_name)
            .bind(redirect_uris)
            .execute(&pool)
            .await?;
            Ok(())
        }
    }

    fn lookup_client(
        &self,
        client_id: String,
    ) -> impl Future<Output = Result<Option<McpOAuthClient>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let Some(row) = sqlx::query(
                "SELECT client_id, display_name, redirect_uris_json FROM mcp_oauth_clients WHERE client_id = ?",
            )
            .bind(client_id)
            .fetch_optional(&pool)
            .await? else {
                return Ok(None);
            };
            Ok(Some(McpOAuthClient {
                client_id: row.try_get("client_id")?,
                display_name: row.try_get("display_name")?,
                redirect_uris: serde_json::from_str(
                    &row.try_get::<String, _>("redirect_uris_json")?,
                )?,
            }))
        }
    }

    fn issue_authorization_code(
        &self,
        code: String,
        grant: McpAuthorizationGrant,
        code_challenge: String,
        expires_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO mcp_authorization_codes
                 (code_hash, user_id, client_id, redirect_uri, resource_uri, scopes, code_challenge, expires_at_ms)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(hash_token(&code))
            .bind(grant.user_id.to_string())
            .bind(grant.client_id)
            .bind(grant.redirect_uri)
            .bind(grant.resource_uri)
            .bind(grant.scopes.join(" "))
            .bind(code_challenge)
            .bind(expires_at.get())
            .execute(&pool)
            .await?;
            Ok(())
        }
    }

    fn consume_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: String,
        resource_uri: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<(McpAuthorizationGrant, String)>, Self::Error>> + Send
    {
        let pool = self.pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            let row = sqlx::query(
                "SELECT user_id, client_id, redirect_uri, resource_uri, scopes, code_challenge
                 FROM mcp_authorization_codes
                 WHERE code_hash = ? AND client_id = ? AND redirect_uri = ? AND resource_uri = ?
                   AND consumed_at_ms IS NULL AND expires_at_ms > ?",
            )
            .bind(hash_token(&code))
            .bind(&client_id)
            .bind(&redirect_uri)
            .bind(&resource_uri)
            .bind(now.get())
            .fetch_optional(&mut *transaction)
            .await?;
            let Some(row) = row else {
                transaction.rollback().await?;
                return Ok(None);
            };
            let updated = sqlx::query(
                "UPDATE mcp_authorization_codes SET consumed_at_ms = ?
                 WHERE code_hash = ? AND consumed_at_ms IS NULL",
            )
            .bind(now.get())
            .bind(hash_token(&code))
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if updated != 1 {
                transaction.rollback().await?;
                return Ok(None);
            }
            let user_id = EntityId::from_str(&row.try_get::<String, _>("user_id")?)
                .map_err(|_| McpOAuthStoreError::CorruptUser)?;
            let scopes = row
                .try_get::<String, _>("scopes")?
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            let grant = McpAuthorizationGrant {
                user_id: UserId::new(user_id),
                client_id: row.try_get("client_id")?,
                redirect_uri: row.try_get("redirect_uri")?,
                resource_uri: row.try_get("resource_uri")?,
                scopes,
            };
            let challenge = row.try_get("code_challenge")?;
            transaction.commit().await?;
            Ok(Some((grant, challenge)))
        }
    }

    fn issue_token_pair(
        &self,
        access_token: String,
        refresh_token: String,
        grant: McpAuthorizationGrant,
        access_expires_at: UnixMillis,
        refresh_expires_at: UnixMillis,
        issued_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            let scopes = grant.scopes.join(" ");
            sqlx::query(
                "INSERT INTO mcp_access_tokens
                 (token_hash, user_id, client_id, resource_uri, scopes, expires_at_ms, issued_at_ms)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(hash_token(&access_token))
            .bind(grant.user_id.to_string())
            .bind(&grant.client_id)
            .bind(&grant.resource_uri)
            .bind(&scopes)
            .bind(access_expires_at.get())
            .bind(issued_at.get())
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "INSERT INTO mcp_refresh_tokens
                 (token_hash, user_id, client_id, resource_uri, scopes, expires_at_ms, issued_at_ms)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(hash_token(&refresh_token))
            .bind(grant.user_id.to_string())
            .bind(&grant.client_id)
            .bind(&grant.resource_uri)
            .bind(scopes)
            .bind(refresh_expires_at.get())
            .bind(issued_at.get())
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            Ok(())
        }
    }

    fn revoke_client_tokens(
        &self,
        user_id: UserId,
        client_id: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            sqlx::query(
                "UPDATE mcp_access_tokens SET revoked_at_ms = ?
                 WHERE user_id = ? AND client_id = ? AND revoked_at_ms IS NULL",
            )
            .bind(now.get())
            .bind(user_id.to_string())
            .bind(&client_id)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "UPDATE mcp_refresh_tokens SET revoked_at_ms = ?
                 WHERE user_id = ? AND client_id = ? AND revoked_at_ms IS NULL",
            )
            .bind(now.get())
            .bind(user_id.to_string())
            .bind(&client_id)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            Ok(())
        }
    }

    fn list_client_authorizations(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<McpClientAuthorization>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let rows = sqlx::query(
                "SELECT refresh.client_id, clients.display_name,
                        GROUP_CONCAT(DISTINCT refresh.scopes) AS scopes,
                        MIN(refresh.issued_at_ms) AS authorized_at_ms,
                        MAX(access.last_used_at_ms) AS last_used_at_ms
                 FROM mcp_refresh_tokens AS refresh
                 JOIN mcp_oauth_clients AS clients ON clients.client_id = refresh.client_id
                 LEFT JOIN mcp_access_tokens AS access
                   ON access.user_id = refresh.user_id
                  AND access.client_id = refresh.client_id
                  AND access.revoked_at_ms IS NULL
                 WHERE refresh.user_id = ? AND refresh.revoked_at_ms IS NULL
                 GROUP BY refresh.client_id, clients.display_name
                 ORDER BY authorized_at_ms DESC, refresh.client_id ASC",
            )
            .bind(user_id.to_string())
            .fetch_all(&pool)
            .await?;
            rows.into_iter()
                .map(|row| {
                    let mut scopes = row
                        .try_get::<String, _>("scopes")?
                        .split(',')
                        .flat_map(str::split_whitespace)
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    scopes.sort();
                    scopes.dedup();
                    Ok(McpClientAuthorization {
                        client_id: row.try_get("client_id")?,
                        display_name: row.try_get("display_name")?,
                        scopes,
                        authorized_at: UnixMillis::new(row.try_get("authorized_at_ms")?),
                        last_used_at: row
                            .try_get::<Option<i64>, _>("last_used_at_ms")?
                            .map(UnixMillis::new),
                    })
                })
                .collect()
        }
    }

    fn rotate_refresh_token(
        &self,
        rotation: McpRefreshTokenRotation,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<McpAuthorizationGrant>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let McpRefreshTokenRotation {
                refresh_token,
                client_id,
                resource_uri,
                new_access_token,
                new_refresh_token,
                access_expires_at,
                refresh_expires_at,
            } = rotation;
            let mut transaction = pool.begin().await?;
            let row = sqlx::query(
                "SELECT user_id, client_id, resource_uri, scopes
                 FROM mcp_refresh_tokens
                 WHERE token_hash = ? AND client_id = ? AND resource_uri = ?
                   AND rotated_at_ms IS NULL AND revoked_at_ms IS NULL AND expires_at_ms > ?",
            )
            .bind(hash_token(&refresh_token))
            .bind(&client_id)
            .bind(&resource_uri)
            .bind(now.get())
            .fetch_optional(&mut *transaction)
            .await?;
            let Some(row) = row else {
                transaction.rollback().await?;
                return Ok(None);
            };
            let updated = sqlx::query(
                "UPDATE mcp_refresh_tokens SET rotated_at_ms = ?
                 WHERE token_hash = ? AND rotated_at_ms IS NULL AND revoked_at_ms IS NULL",
            )
            .bind(now.get())
            .bind(hash_token(&refresh_token))
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if updated != 1 {
                transaction.rollback().await?;
                return Ok(None);
            }
            let user_id = EntityId::from_str(&row.try_get::<String, _>("user_id")?)
                .map_err(|_| McpOAuthStoreError::CorruptUser)?;
            let scopes = row
                .try_get::<String, _>("scopes")?
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let grant = McpAuthorizationGrant {
                user_id: UserId::new(user_id),
                client_id: row.try_get("client_id")?,
                redirect_uri: String::new(),
                resource_uri: row.try_get("resource_uri")?,
                scopes,
            };
            let scopes = grant.scopes.join(" ");
            sqlx::query(
                "INSERT INTO mcp_access_tokens
                 (token_hash, user_id, client_id, resource_uri, scopes, expires_at_ms, issued_at_ms)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(hash_token(&new_access_token))
            .bind(grant.user_id.to_string())
            .bind(&grant.client_id)
            .bind(&grant.resource_uri)
            .bind(&scopes)
            .bind(access_expires_at.get())
            .bind(now.get())
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "INSERT INTO mcp_refresh_tokens
                 (token_hash, user_id, client_id, resource_uri, scopes, expires_at_ms, issued_at_ms)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(hash_token(&new_refresh_token))
            .bind(grant.user_id.to_string())
            .bind(&grant.client_id)
            .bind(&grant.resource_uri)
            .bind(scopes)
            .bind(refresh_expires_at.get())
            .bind(now.get())
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            Ok(Some(grant))
        }
    }
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
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM notes WHERE note_id = ?)")
                    .bind(projection.note_id.to_string())
                    .fetch_one(&mut *transaction)
                    .await?;
            sqlx::query("DELETE FROM note_search WHERE note_id = ?")
                .bind(projection.note_id.to_string())
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
            sqlx::query("DELETE FROM note_tags WHERE note_id = ?")
                .bind(projection.note_id.to_string())
                .execute(&mut *transaction)
                .await?;
            insert_note_projection_rows(&mut transaction, &projection, revision).await?;
            if !exists {
                sqlx::query("INSERT INTO note_acl (note_id, user_id, permission) VALUES (?, ?, 3)")
                    .bind(projection.note_id.to_string())
                    .bind(projection.owner_id.to_string())
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
            sqlx::query("DELETE FROM note_search WHERE note_id = ?")
                .bind(note_id.to_string())
                .execute(&pool)
                .await?;
            sqlx::query("DELETE FROM notes WHERE note_id = ?")
                .bind(note_id.to_string())
                .execute(&pool)
                .await?;
            Ok(())
        }
    }
}

/// `notes`、検索、anchor、xref投影の共通挿入処理。
///
/// 呼出側は、既存projectionを置換する場合に検索・anchor・xrefを先に削除する。ACL初期化は
/// この操作に含めず、新規ノート作成を検出した通常保存経路だけが行う。
async fn insert_note_projection_rows(
    connection: &mut sqlx::SqliteConnection,
    projection: &NoteProjection,
    revision: SourceRevision,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO notes (note_id, relative_path, title, creator_id, created_at, updated_at, source_revision, deleted_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
         ON CONFLICT(note_id) DO UPDATE SET
           relative_path = excluded.relative_path, title = excluded.title,
           creator_id = excluded.creator_id, created_at = excluded.created_at, updated_at = excluded.updated_at,
           source_revision = excluded.source_revision, deleted_at_ms = NULL",
    )
    .bind(projection.note_id.to_string())
    .bind(format!("notes/{}.adoc", projection.note_id))
    .bind(&projection.title)
    .bind(projection.owner_id.to_string())
    .bind(&projection.created_at)
    .bind(&projection.updated_at)
    .bind(revision.bytes().to_vec())
    .execute(&mut *connection)
    .await?;
    sqlx::query("INSERT INTO note_search (note_id, title, content) VALUES (?, ?, ?)")
        .bind(projection.note_id.to_string())
        .bind(&projection.title)
        .bind(&projection.search_text)
        .execute(&mut *connection)
        .await?;
    for tag in &projection.tags {
        sqlx::query("INSERT INTO note_tags (note_id, tag_key) VALUES (?, ?)")
            .bind(projection.note_id.to_string())
            .bind(tag)
            .execute(&mut *connection)
            .await?;
    }
    for anchor in &projection.anchors {
        sqlx::query("INSERT INTO note_anchors (note_id, anchor_id) VALUES (?, ?)")
            .bind(projection.note_id.to_string())
            .bind(anchor)
            .execute(&mut *connection)
            .await?;
    }
    for reference in &projection.references {
        sqlx::query(
            "INSERT INTO note_references
             (source_note_id, source_start, source_end, target_note_id, target_anchor)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(projection.note_id.to_string())
        .bind(i64::from(reference.source_start))
        .bind(i64::from(reference.source_end))
        .bind(&reference.target_note_id)
        .bind(&reference.target_anchor)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

impl NoteQueryStore for SqliteNoteQueryStore {
    type Error = NoteQueryStoreError;

    fn list_visible(
        &self,
        actor: Actor,
        offset: u64,
        limit: u32,
    ) -> impl Future<Output = Result<NotePage, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let rows = sqlx::query(
                "SELECT notes.note_id, notes.title FROM notes
                 WHERE ? OR EXISTS (
                   SELECT 1 FROM note_acl
                   WHERE note_acl.note_id = notes.note_id AND note_acl.user_id = ?
                 )
                 ORDER BY notes.title COLLATE NOCASE ASC, notes.note_id ASC
                 LIMIT ? OFFSET ?",
            )
            .bind(actor.is_root)
            .bind(actor.user_id.to_string())
            .bind(i64::from(limit) + 1)
            .bind(i64::try_from(offset).unwrap_or(i64::MAX))
            .fetch_all(&pool)
            .await?;
            let has_next = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
            let notes = rows
                .into_iter()
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .map(|row| note_summary_from_row(&row))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(NotePage {
                next_offset: has_next
                    .then(|| offset.checked_add(u64::from(limit)))
                    .flatten(),
                notes,
            })
        }
    }

    fn search_visible(
        &self,
        actor: Actor,
        query: String,
        filters: marginalis_domain::NoteSearchFilters,
        offset: u64,
        limit: u32,
    ) -> impl Future<Output = Result<NotePage, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let mut statement = QueryBuilder::<Sqlite>::new(
                "SELECT notes.note_id, notes.title FROM note_search JOIN notes ON notes.note_id = note_search.note_id WHERE note_search MATCH ",
            );
            statement.push_bind(fts_phrase_query(&query));
            statement.push(" AND (").push_bind(actor.is_root).push(" OR EXISTS (SELECT 1 FROM note_acl WHERE note_acl.note_id = notes.note_id AND note_acl.user_id = ").push_bind(actor.user_id.to_string()).push("))");
            if let Some(creator) = filters.creator_id {
                statement
                    .push(" AND notes.creator_id = ")
                    .push_bind(creator.to_string());
            }
            for tag in filters.tags {
                statement.push(" AND EXISTS (SELECT 1 FROM note_tags WHERE note_tags.note_id = notes.note_id AND note_tags.tag_key = ").push_bind(tag).push(")");
            }
            if let Some(value) = filters.created_after {
                statement.push(" AND notes.created_at >= ").push_bind(value);
            }
            if let Some(value) = filters.created_before {
                statement.push(" AND notes.created_at <= ").push_bind(value);
            }
            if let Some(value) = filters.updated_after {
                statement.push(" AND notes.updated_at >= ").push_bind(value);
            }
            if let Some(value) = filters.updated_before {
                statement.push(" AND notes.updated_at <= ").push_bind(value);
            }
            if let Some(note_id) = filters.links_to {
                statement.push(" AND EXISTS (SELECT 1 FROM note_references WHERE note_references.source_note_id = notes.note_id AND note_references.target_note_id = ").push_bind(note_id.to_string()).push(")");
            }
            if let Some(note_id) = filters.linked_from {
                statement.push(" AND EXISTS (SELECT 1 FROM note_references WHERE note_references.source_note_id = ").push_bind(note_id.to_string()).push(" AND note_references.target_note_id = notes.note_id)");
            }
            statement
                .push(" ORDER BY bm25(note_search, 0.0, 100.0, 1.0), notes.note_id ASC LIMIT ")
                .push_bind(i64::from(limit) + 1)
                .push(" OFFSET ")
                .push_bind(i64::try_from(offset).unwrap_or(i64::MAX));
            let rows = statement.build().fetch_all(&pool).await?;
            let has_next = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
            let notes = rows
                .into_iter()
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .map(|row| note_summary_from_row(&row))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(NotePage {
                next_offset: has_next
                    .then(|| offset.checked_add(u64::from(limit)))
                    .flatten(),
                notes,
            })
        }
    }

    fn list_visible_links(
        &self,
        actor: Actor,
        note_id: NoteId,
        offset: u64,
        limit: u32,
    ) -> impl Future<Output = Result<NoteLinkPage, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let rows = sqlx::query(
                "SELECT refs.source_start, refs.source_end, refs.target_anchor,
                        target.note_id AS target_note_id, target.title AS target_title
                 FROM note_references AS refs
                 JOIN notes AS source ON source.note_id = refs.source_note_id
                 JOIN notes AS target ON target.note_id = refs.target_note_id
                 WHERE refs.source_note_id = ?
                   AND (? OR EXISTS (
                     SELECT 1 FROM note_acl
                     WHERE note_acl.note_id = source.note_id AND note_acl.user_id = ?
                   ))
                   AND (? OR EXISTS (
                     SELECT 1 FROM note_acl
                     WHERE note_acl.note_id = target.note_id AND note_acl.user_id = ?
                   ))
                 ORDER BY refs.source_start ASC, refs.source_end ASC,
                          target.note_id ASC
                 LIMIT ? OFFSET ?",
            )
            .bind(note_id.to_string())
            .bind(actor.is_root)
            .bind(actor.user_id.to_string())
            .bind(actor.is_root)
            .bind(actor.user_id.to_string())
            .bind(i64::from(limit) + 1)
            .bind(i64::try_from(offset).unwrap_or(i64::MAX))
            .fetch_all(&pool)
            .await?;
            let has_next = rows.len() > usize::try_from(limit).unwrap_or(usize::MAX);
            let links = rows
                .into_iter()
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .map(|row| -> Result<NoteLink, NoteQueryStoreError> {
                    let target_note_id: String = row.try_get("target_note_id")?;
                    let target = NoteSummary {
                        note_id: NoteId::new(
                            EntityId::from_str(&target_note_id)
                                .map_err(|_| NoteQueryStoreError::CorruptNote)?,
                        ),
                        title: row.try_get("target_title")?,
                    };
                    Ok(NoteLink {
                        source_start: u32::try_from(row.try_get::<i64, _>("source_start")?)
                            .map_err(|_| NoteQueryStoreError::CorruptNote)?,
                        source_end: u32::try_from(row.try_get::<i64, _>("source_end")?)
                            .map_err(|_| NoteQueryStoreError::CorruptNote)?,
                        target,
                        target_anchor: row.try_get("target_anchor")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(NoteLinkPage {
                links,
                next_offset: has_next
                    .then(|| offset.checked_add(u64::from(limit)))
                    .flatten(),
            })
        }
    }
}

fn note_summary_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<NoteSummary, NoteQueryStoreError> {
    let note_id: String = row.try_get("note_id")?;
    Ok(NoteSummary {
        note_id: NoteId::new(
            EntityId::from_str(&note_id).map_err(|_| NoteQueryStoreError::CorruptNote)?,
        ),
        title: row.try_get("title")?,
    })
}

/// 利用者入力をFTS演算子として解釈せず、一つのphraseとして検索する。
fn fts_phrase_query(query: &str) -> String {
    format!("\"{}\"", query.replace('"', "\"\""))
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

    fn cancel(
        &self,
        operation_id: OperationId,
        updated_at: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let result = sqlx::query(
                "UPDATE operation_journal SET state = 'completed', updated_at_ms = ?
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
        JournalEntry, McpAccessTokenStore, McpOAuthStore, NoteAclService, NoteAclServiceError,
        NoteAclStore, NoteOperationKind, NoteProjectionStore, NoteQueryStore, OidcIdentityStore,
        OidcLoginAttempt, OidcLoginAttemptStore, OidcUserAdministrationStore, OperationId,
        OperationJournal, OperationState, RootCredentialStore,
    };
    use marginalis_domain::{
        Actor, EntityId, McpAuthorizationGrant, McpOAuthClient, NoteId, NotePermission,
        NoteProjection, NoteReference, OidcIdentity, OidcLoginResult, OidcUser, RegistrationPolicy,
        RootAuditAction, RootAuditEvent, SourceRevision, UnixMillis, UserId, UserStatus,
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
    async fn backup_writes_a_readable_consistent_sqlite_database() {
        let suffix: u64 = rand::random();
        let source_path =
            std::env::temp_dir().join(format!("marginalis-backup-source-{suffix}.sqlite"));
        let backup_path =
            std::env::temp_dir().join(format!("marginalis-backup-output-{suffix}.sqlite"));
        let source_url = format!("sqlite:{}", source_path.display());
        let database = SqliteDatabase::connect(&source_url)
            .await
            .expect("database");
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES ('01800000-0000-7000-8000-000000000001', 'oidc', 'active', 'Backup user', 0, 0)",
        )
        .execute(database.pool())
        .await
        .expect("insert user");
        database
            .backup_to(backup_path.to_str().expect("UTF-8 backup path"))
            .await
            .expect("backup");
        drop(database);
        SqliteDatabase::validate_backup_file(&backup_path)
            .await
            .expect("validate backup");

        let backup_url = format!("sqlite:{}", backup_path.display());
        let backup = SqliteDatabase::connect(&backup_url)
            .await
            .expect("open backup");
        let count = sqlx::query("SELECT count(*) AS count FROM users")
            .fetch_one(backup.pool())
            .await
            .expect("query backup")
            .get::<i64, _>("count");
        assert_eq!(count, 1);
        drop(backup);
        std::fs::remove_file(source_path).expect("remove source");
        std::fs::remove_file(backup_path).expect("remove backup");
    }

    #[tokio::test]
    async fn initial_registration_policy_is_applied_only_to_a_new_database() {
        let suffix: u64 = rand::random();
        let path = std::env::temp_dir().join(format!("marginalis-policy-{suffix}.sqlite"));
        let url = format!("sqlite:{}", path.display());
        let database = SqliteDatabase::connect_with_initial_registration_policy(
            &url,
            RegistrationPolicy::Open,
        )
        .await
        .expect("new database");
        assert_eq!(
            database.registration_policy().await.expect("policy"),
            RegistrationPolicy::Open
        );
        database
            .set_registration_policy(RegistrationPolicy::Approval)
            .await
            .expect("change policy");
        drop(database);
        let database = SqliteDatabase::connect_with_initial_registration_policy(
            &url,
            RegistrationPolicy::Open,
        )
        .await
        .expect("existing database");
        assert_eq!(
            database
                .registration_policy()
                .await
                .expect("preserved policy"),
            RegistrationPolicy::Approval
        );
        drop(database);
        std::fs::remove_file(path).expect("remove database");
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
        assert_eq!(version, 12);
        let index: String = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name = 'notes_live_title_idx'",
        )
        .fetch_one(&pool)
        .await
        .expect("index")
        .try_get("name")
        .expect("name");
        assert_eq!(index, "notes_live_title_idx");
        let search_table: String = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'note_search'",
        )
        .fetch_one(&pool)
        .await
        .expect("search table")
        .try_get("name")
        .expect("name");
        assert_eq!(search_table, "note_search");
        let token_table: String = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'mcp_access_tokens'",
        )
        .fetch_one(&pool)
        .await
        .expect("MCP token table")
        .try_get("name")
        .expect("table name");
        assert_eq!(token_table, "mcp_access_tokens");
        let audit_table: String = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'root_audit_log'",
        )
        .fetch_one(&pool)
        .await
        .expect("audit table")
        .try_get("name")
        .expect("table name");
        assert_eq!(audit_table, "root_audit_log");
    }

    #[tokio::test]
    async fn root_audit_is_secret_free_and_purged_by_retention_cutoff() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let root = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000090").expect("UUIDv7"),
        );
        database
            .record_root_audit(RootAuditEvent {
                action: RootAuditAction::LoginSucceeded,
                actor_user_id: Some(root),
                target_user_id: None,
                target: None,
                occurred_at: UnixMillis::new(10),
            })
            .await
            .expect("record audit");
        database
            .record_root_audit(RootAuditEvent {
                action: RootAuditAction::RegistrationPolicyChanged,
                actor_user_id: Some(root),
                target_user_id: None,
                target: Some("approval".into()),
                occurred_at: UnixMillis::new(20),
            })
            .await
            .expect("record audit");
        assert_eq!(
            database
                .purge_root_audit_before(UnixMillis::new(20))
                .await
                .expect("purge audit"),
            1
        );
        let row = sqlx::query("SELECT action, target FROM root_audit_log")
            .fetch_one(database.pool())
            .await
            .expect("remaining audit");
        assert_eq!(
            row.get::<String, _>("action"),
            "registration-policy-changed"
        );
        assert_eq!(row.get::<String, _>("target"), "approval");
    }

    #[tokio::test]
    async fn maintenance_purges_expired_ephemera_without_touching_root_audit() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let user_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000088").expect("user ID"),
        );
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'User', 0, 0)",
        )
        .bind(user_id.to_string())
        .execute(database.pool())
        .await
        .expect("user");
        sqlx::query(
            "INSERT INTO oidc_login_attempts (state_hash, nonce, pkce_verifier, expires_at_ms)
             VALUES (X'01', 'nonce', 'verifier', 10)",
        )
        .execute(database.pool())
        .await
        .expect("OIDC attempt");
        sqlx::query(
            "INSERT INTO web_sessions
             (session_id_hash, csrf_token_hash, user_id, idle_timeout_ms, issued_at_ms, last_seen_at_ms, idle_expires_at_ms, absolute_expires_at_ms)
             VALUES (X'02', X'03', ?, 1, 0, 0, 10, 10)",
        )
        .bind(user_id.to_string())
        .execute(database.pool())
        .await
        .expect("session");
        database
            .record_root_audit(RootAuditEvent {
                action: RootAuditAction::LoginSucceeded,
                actor_user_id: None,
                target_user_id: None,
                target: None,
                occurred_at: UnixMillis::new(0),
            })
            .await
            .expect("audit");
        assert_eq!(
            database
                .purge_expired_ephemera(UnixMillis::new(10))
                .await
                .expect("purge"),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM root_audit_log")
                .fetch_one(database.pool())
                .await
                .expect("audit count"),
            1
        );
    }

    #[tokio::test]
    async fn all_projection_rebuild_replaces_search_graph_and_removed_notes_atomically() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let owner = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000091").expect("UUIDv7"),
        );
        database
            .root_credential_store()
            .initialize_if_missing("root-password".into(), owner, UnixMillis::new(0))
            .await
            .expect("owner");
        let note_id = NoteId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000092").expect("UUIDv7"),
        );
        let projection = NoteProjection {
            note_id,
            owner_id: owner,
            title: "Rebuilt".into(),
            tags: Vec::new(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
            search_text: "rebuild search text".into(),
            anchors: vec!["start".into()],
            references: vec![NoteReference {
                source_start: 3,
                source_end: 9,
                target_note_id: "01800000-0000-7000-8000-000000000093".into(),
                target_anchor: Some("target".into()),
            }],
        };
        database
            .replace_all_note_projections(&[(projection, SourceRevision::from_source(b"source"))])
            .await
            .expect("replace all projections");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM note_search")
                .fetch_one(database.pool())
                .await
                .expect("search count"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM note_references")
                .fetch_one(database.pool())
                .await
                .expect("reference count"),
            1
        );
        database
            .replace_all_note_projections(&[])
            .await
            .expect("remove stale projections");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM notes")
                .fetch_one(database.pool())
                .await
                .expect("note count"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM note_acl")
                .fetch_one(database.pool())
                .await
                .expect("ACL count"),
            0
        );
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
        let target_note_id = NoteId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000012")
                .expect("UUIDv7 target note ID"),
        );
        let target_owner_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000013")
                .expect("UUIDv7 target owner ID"),
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
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'Target owner', 0, 0)",
        )
        .bind(target_owner_id.to_string())
        .execute(database.pool())
        .await
        .expect("insert target owner");
        database
            .note_projection_store()
            .replace_projection(
                NoteProjection {
                    note_id: target_note_id,
                    owner_id: target_owner_id,
                    title: "Target".into(),
                    tags: Vec::new(),
                    created_at: "2026-01-01T00:00:00.000Z".into(),
                    updated_at: "2026-01-01T00:00:00.000Z".into(),
                    search_text: "Target".into(),
                    anchors: vec!["target".into()],
                    references: Vec::new(),
                },
                SourceRevision::from_source(b"= Target\n"),
            )
            .await
            .expect("store target projection");
        database
            .note_projection_store()
            .replace_projection(
                NoteProjection {
                    note_id,
                    owner_id,
                    title: "Projection".into(),
                    tags: Vec::new(),
                    created_at: "2026-01-01T00:00:00.000Z".into(),
                    updated_at: "2026-01-01T00:00:00.000Z".into(),
                    search_text: "Projection".into(),
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
        let actor = Actor {
            user_id: owner_id,
            is_root: false,
        };
        assert!(
            database
                .note_query_store()
                .list_visible_links(actor, note_id, 0, 10)
                .await
                .expect("private target is hidden")
                .links
                .is_empty()
        );
        database
            .note_acl_store()
            .set_permission(target_note_id, owner_id, Some(NotePermission::Read))
            .await
            .expect("share target");
        let links = database
            .note_query_store()
            .list_visible_links(actor, note_id, 0, 10)
            .await
            .expect("visible target link");
        assert_eq!(links.links.len(), 1);
        assert_eq!(links.links[0].target.note_id, target_note_id);
        assert_eq!(links.links[0].target_anchor.as_deref(), Some("target"));
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
                    tags: Vec::new(),
                    created_at: "2026-01-01T00:00:00.000Z".into(),
                    updated_at: "2026-01-01T00:00:00.000Z".into(),
                    search_text: "ACL".into(),
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
        acl.set_permission(note_id, other_id, Some(NotePermission::Admin))
            .await
            .expect("add second admin");
        acl.set_permission(note_id, owner_id, Some(NotePermission::Read))
            .await
            .expect("demote former owner");
        database
            .note_projection_store()
            .replace_projection(
                NoteProjection {
                    note_id,
                    owner_id,
                    title: "Updated ACL".into(),
                    tags: Vec::new(),
                    created_at: "2026-01-01T00:00:00.000Z".into(),
                    updated_at: "2026-01-01T00:00:00.000Z".into(),
                    search_text: "updated ACL".into(),
                    anchors: Vec::new(),
                    references: Vec::new(),
                },
                SourceRevision::from_source(b"= Updated ACL\n"),
            )
            .await
            .expect("update projection");
        assert_eq!(
            acl.permission_for(
                Actor {
                    user_id: owner_id,
                    is_root: false,
                },
                note_id,
            )
            .await
            .expect("former owner permission"),
            Some(NotePermission::Read)
        );
        database
            .replace_all_note_projections(&[(
                NoteProjection {
                    note_id,
                    owner_id,
                    title: "Rebuilt ACL".into(),
                    tags: Vec::new(),
                    created_at: "2026-01-01T00:00:00.000Z".into(),
                    updated_at: "2026-01-01T00:00:00.000Z".into(),
                    search_text: "rebuilt ACL".into(),
                    anchors: Vec::new(),
                    references: Vec::new(),
                },
                SourceRevision::from_source(b"= Rebuilt ACL\n"),
            )])
            .await
            .expect("rebuild projections");
        assert_eq!(
            acl.permission_for(
                Actor {
                    user_id: owner_id,
                    is_root: false,
                },
                note_id,
            )
            .await
            .expect("rebuilt former owner permission"),
            Some(NotePermission::Read)
        );
        assert!(
            !database
                .oidc_user_administration_store()
                .disable(other_id, UnixMillis::new(1))
                .await
                .expect("reject disabling the only active admin")
        );
        acl.set_permission(note_id, owner_id, Some(NotePermission::Admin))
            .await
            .expect("restore a second active admin");
        assert!(
            database
                .oidc_user_administration_store()
                .disable(other_id, UnixMillis::new(2))
                .await
                .expect("disable user after authority transfer")
        );
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

    #[tokio::test]
    async fn search_filters_notes_before_returning_results() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let owner_id =
            UserId::new(EntityId::from_str("01800000-0000-7000-8000-000000000060").expect("owner"));
        let other_id =
            UserId::new(EntityId::from_str("01800000-0000-7000-8000-000000000061").expect("other"));
        let note_id =
            NoteId::new(EntityId::from_str("01800000-0000-7000-8000-000000000062").expect("note"));
        for user_id in [owner_id, other_id] {
            sqlx::query(
                "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
                 VALUES (?, 'oidc', 'active', 'User', 0, 0)",
            )
            .bind(user_id.to_string())
            .execute(database.pool())
            .await
            .expect("user");
        }
        database
            .note_projection_store()
            .replace_projection(
                NoteProjection {
                    note_id,
                    owner_id,
                    title: "Private hypothesis".into(),
                    tags: Vec::new(),
                    created_at: "2026-01-01T00:00:00.000Z".into(),
                    updated_at: "2026-01-01T00:00:00.000Z".into(),
                    search_text: "unique-secret-phrase".into(),
                    anchors: Vec::new(),
                    references: Vec::new(),
                },
                SourceRevision::from_source(b"unique-secret-phrase"),
            )
            .await
            .expect("projection");
        let query = database.note_query_store();
        let owner_results = query
            .search_visible(
                Actor {
                    user_id: owner_id,
                    is_root: false,
                },
                "unique-secret-phrase".into(),
                marginalis_domain::NoteSearchFilters::default(),
                0,
                10,
            )
            .await
            .expect("owner search");
        assert_eq!(owner_results.notes.len(), 1);
        let other_results = query
            .search_visible(
                Actor {
                    user_id: other_id,
                    is_root: false,
                },
                "unique-secret-phrase".into(),
                marginalis_domain::NoteSearchFilters::default(),
                0,
                10,
            )
            .await
            .expect("other search");
        assert!(other_results.notes.is_empty());
    }

    #[tokio::test]
    async fn search_ranks_title_matches_before_body_matches() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let owner_id =
            UserId::new(EntityId::from_str("01800000-0000-7000-8000-000000000063").expect("owner"));
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'User', 0, 0)",
        )
        .bind(owner_id.to_string())
        .execute(database.pool())
        .await
        .expect("user");
        let title_note = NoteId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000064").expect("title note"),
        );
        let body_note = NoteId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000065").expect("body note"),
        );
        for (note_id, title, search_text) in [
            (title_note, "Needle in title", "ordinary body"),
            (body_note, "Ordinary title", "needle needle needle"),
        ] {
            database
                .note_projection_store()
                .replace_projection(
                    NoteProjection {
                        note_id,
                        owner_id,
                        title: title.into(),
                        tags: Vec::new(),
                        created_at: "2026-01-01T00:00:00.000Z".into(),
                        updated_at: "2026-01-01T00:00:00.000Z".into(),
                        search_text: search_text.into(),
                        anchors: Vec::new(),
                        references: Vec::new(),
                    },
                    SourceRevision::from_source(search_text.as_bytes()),
                )
                .await
                .expect("projection");
        }
        let results = database
            .note_query_store()
            .search_visible(
                Actor {
                    user_id: owner_id,
                    is_root: false,
                },
                "needle".into(),
                marginalis_domain::NoteSearchFilters::default(),
                0,
                10,
            )
            .await
            .expect("search");
        assert_eq!(results.notes[0].note_id, title_note);
    }

    #[tokio::test]
    async fn mcp_access_token_requires_matching_resource_scope_and_active_user() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let user_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000071").expect("user ID"),
        );
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'User', 0, 0)",
        )
        .bind(user_id.to_string())
        .execute(database.pool())
        .await
        .expect("user");
        sqlx::query(
            "INSERT INTO mcp_access_tokens (token_hash, user_id, resource_uri, scopes, expires_at_ms)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(hash_token("opaque-token"))
        .bind(user_id.to_string())
        .bind("https://example.test/mcp")
        .bind("notes:read")
        .bind(1_000_i64)
        .execute(database.pool())
        .await
        .expect("token");
        let store = database.mcp_access_token_store();
        assert_eq!(
            store
                .authenticate(
                    "opaque-token".into(),
                    "https://example.test/mcp".into(),
                    "notes:read".into(),
                    UnixMillis::new(1),
                )
                .await
                .expect("authentication"),
            Some(Actor {
                user_id,
                is_root: false,
            })
        );
        assert!(
            store
                .authenticate(
                    "opaque-token".into(),
                    "https://other.test/mcp".into(),
                    "notes:read".into(),
                    UnixMillis::new(1),
                )
                .await
                .expect("authentication")
                .is_none()
        );
        assert!(
            store
                .authenticate(
                    "opaque-token".into(),
                    "https://example.test/mcp".into(),
                    "notes:delete".into(),
                    UnixMillis::new(1),
                )
                .await
                .expect("scope authentication")
                .is_none()
        );
    }

    #[tokio::test]
    async fn mcp_authorization_codes_are_client_bound_and_single_use() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let user_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000072").expect("user ID"),
        );
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'User', 0, 0)",
        )
        .bind(user_id.to_string())
        .execute(database.pool())
        .await
        .expect("user");
        let store = database.mcp_oauth_store();
        store
            .upsert_client(McpOAuthClient {
                client_id: "client".into(),
                display_name: "Client".into(),
                redirect_uris: vec!["http://127.0.0.1:4567/callback".into()],
            })
            .await
            .expect("client");
        let grant = McpAuthorizationGrant {
            user_id,
            client_id: "client".into(),
            redirect_uri: "http://127.0.0.1:4567/callback".into(),
            resource_uri: "https://example.test/mcp".into(),
            scopes: vec!["notes:read".into()],
        };
        store
            .issue_authorization_code(
                "code".into(),
                grant.clone(),
                "challenge".into(),
                UnixMillis::new(100),
            )
            .await
            .expect("code");
        assert_eq!(
            store
                .consume_authorization_code(
                    "code".into(),
                    "client".into(),
                    "http://127.0.0.1:4567/callback".into(),
                    "https://example.test/mcp".into(),
                    UnixMillis::new(1),
                )
                .await
                .expect("consume"),
            Some((grant, "challenge".into()))
        );
        assert!(
            store
                .consume_authorization_code(
                    "code".into(),
                    "client".into(),
                    "http://127.0.0.1:4567/callback".into(),
                    "https://example.test/mcp".into(),
                    UnixMillis::new(2),
                )
                .await
                .expect("second consume")
                .is_none()
        );
    }

    #[tokio::test]
    async fn mcp_refresh_tokens_rotate_once_and_issue_a_new_access_token() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let user_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000073").expect("user ID"),
        );
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'User', 0, 0)",
        )
        .bind(user_id.to_string())
        .execute(database.pool())
        .await
        .expect("user");
        let store = database.mcp_oauth_store();
        store
            .upsert_client(McpOAuthClient {
                client_id: "client".into(),
                display_name: "Client".into(),
                redirect_uris: vec!["http://127.0.0.1:4567/callback".into()],
            })
            .await
            .expect("client");
        let grant = McpAuthorizationGrant {
            user_id,
            client_id: "client".into(),
            redirect_uri: "http://127.0.0.1:4567/callback".into(),
            resource_uri: "https://example.test/mcp".into(),
            scopes: vec!["notes:read".into()],
        };
        store
            .issue_token_pair(
                "access-1".into(),
                "refresh-1".into(),
                grant,
                UnixMillis::new(100),
                UnixMillis::new(200),
                UnixMillis::new(0),
            )
            .await
            .expect("tokens");
        assert!(
            store
                .rotate_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "refresh-1".into(),
                        client_id: "client".into(),
                        resource_uri: "https://example.test/mcp".into(),
                        new_access_token: "access-2".into(),
                        new_refresh_token: "refresh-2".into(),
                        access_expires_at: UnixMillis::new(100),
                        refresh_expires_at: UnixMillis::new(200),
                    },
                    UnixMillis::new(1),
                )
                .await
                .expect("rotate")
                .is_some()
        );
        assert!(
            store
                .rotate_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "refresh-1".into(),
                        client_id: "client".into(),
                        resource_uri: "https://example.test/mcp".into(),
                        new_access_token: "access-3".into(),
                        new_refresh_token: "refresh-3".into(),
                        access_expires_at: UnixMillis::new(100),
                        refresh_expires_at: UnixMillis::new(200),
                    },
                    UnixMillis::new(2),
                )
                .await
                .expect("second rotate")
                .is_none()
        );
        assert!(
            database
                .mcp_access_token_store()
                .authenticate(
                    "access-2".into(),
                    "https://example.test/mcp".into(),
                    "notes:read".into(),
                    UnixMillis::new(2),
                )
                .await
                .expect("access token")
                .is_some()
        );
        let authorizations = store
            .list_client_authorizations(user_id)
            .await
            .expect("authorization list");
        assert_eq!(authorizations.len(), 1);
        assert_eq!(authorizations[0].client_id, "client");
        assert_eq!(authorizations[0].scopes, ["notes:read"]);
        assert_eq!(authorizations[0].authorized_at, UnixMillis::new(0));
        assert_eq!(authorizations[0].last_used_at, Some(UnixMillis::new(2)));
        store
            .revoke_client_tokens(user_id, "client".into(), UnixMillis::new(3))
            .await
            .expect("revoke");
        assert!(
            database
                .mcp_access_token_store()
                .authenticate(
                    "access-2".into(),
                    "https://example.test/mcp".into(),
                    "notes:read".into(),
                    UnixMillis::new(4),
                )
                .await
                .expect("revoked access token")
                .is_none()
        );
    }

    #[tokio::test]
    async fn delete_confirmation_is_bound_to_actor_and_single_use() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let user_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000074").expect("user ID"),
        );
        let other_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000075").expect("user ID"),
        );
        let note_id = NoteId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000076").expect("note ID"),
        );
        for id in [user_id, other_id] {
            sqlx::query(
                "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
                 VALUES (?, 'oidc', 'active', 'User', 0, 0)",
            )
            .bind(id.to_string())
            .execute(database.pool())
            .await
            .expect("user");
        }
        database
            .note_projection_store()
            .replace_projection(
                NoteProjection {
                    note_id,
                    owner_id: user_id,
                    title: "Disposable".into(),
                    tags: Vec::new(),
                    created_at: "2026-01-01T00:00:00.000Z".into(),
                    updated_at: "2026-01-01T00:00:00.000Z".into(),
                    search_text: "Disposable".into(),
                    anchors: Vec::new(),
                    references: Vec::new(),
                },
                SourceRevision::from_source(b"= Disposable\n"),
            )
            .await
            .expect("note");
        let revision = SourceRevision::from_source(b"= Disposable\n");
        let store = database.delete_confirmation_store();
        let actor = Actor {
            user_id,
            is_root: false,
        };
        store
            .issue(
                "confirmation".into(),
                actor,
                note_id,
                revision,
                UnixMillis::new(100),
            )
            .await
            .expect("issue");
        assert_eq!(
            store
                .consume(
                    "confirmation".into(),
                    Actor {
                        user_id: other_id,
                        is_root: false
                    },
                    UnixMillis::new(1),
                )
                .await
                .expect("other actor"),
            DeleteConfirmation::Missing
        );
        assert_eq!(
            store
                .consume("confirmation".into(), actor, UnixMillis::new(1))
                .await
                .expect("consume"),
            DeleteConfirmation::Confirmed {
                note_id,
                expected_revision: revision,
            }
        );
        assert_eq!(
            store
                .consume("confirmation".into(), actor, UnixMillis::new(2))
                .await
                .expect("second consume"),
            DeleteConfirmation::Missing
        );
        database
            .note_projection_store()
            .delete_projection(note_id)
            .await
            .expect("delete note projection");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delete_confirmations")
                .fetch_one(database.pool())
                .await
                .expect("confirmation count"),
            0
        );
    }

    #[tokio::test]
    async fn delete_confirmation_becomes_stale_when_an_incoming_reference_changes() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let user_id = UserId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000077").expect("user ID"),
        );
        let target_id = NoteId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000078").expect("target ID"),
        );
        let source_id = NoteId::new(
            EntityId::from_str("01800000-0000-7000-8000-000000000079").expect("source ID"),
        );
        sqlx::query(
            "INSERT INTO users (user_id, authentication_kind, status, display_name, created_at_ms, updated_at_ms)
             VALUES (?, 'oidc', 'active', 'User', 0, 0)",
        )
        .bind(user_id.to_string())
        .execute(database.pool())
        .await
        .expect("user");
        let revision = SourceRevision::from_source(b"= Target\n");
        for (note_id, title) in [(target_id, "Target"), (source_id, "Source")] {
            database
                .note_projection_store()
                .replace_projection(
                    NoteProjection {
                        note_id,
                        owner_id: user_id,
                        title: title.into(),
                        tags: Vec::new(),
                        created_at: "2026-01-01T00:00:00.000Z".into(),
                        updated_at: "2026-01-01T00:00:00.000Z".into(),
                        search_text: title.into(),
                        anchors: Vec::new(),
                        references: Vec::new(),
                    },
                    revision,
                )
                .await
                .expect("projection");
        }
        let actor = Actor {
            user_id,
            is_root: false,
        };
        let store = database.delete_confirmation_store();
        assert_eq!(
            store
                .issue(
                    "stale-confirmation".into(),
                    actor,
                    target_id,
                    revision,
                    UnixMillis::new(100),
                )
                .await
                .expect("issue"),
            0
        );
        database
            .note_projection_store()
            .replace_projection(
                NoteProjection {
                    note_id: source_id,
                    owner_id: user_id,
                    title: "Source".into(),
                    tags: Vec::new(),
                    created_at: "2026-01-01T00:00:00.000Z".into(),
                    updated_at: "2026-01-01T00:00:00.000Z".into(),
                    search_text: "Source".into(),
                    anchors: Vec::new(),
                    references: vec![marginalis_domain::NoteReference {
                        source_start: 0,
                        source_end: 42,
                        target_note_id: target_id.to_string(),
                        target_anchor: None,
                    }],
                },
                revision,
            )
            .await
            .expect("reference update");
        assert_eq!(
            store
                .consume("stale-confirmation".into(), actor, UnixMillis::new(1))
                .await
                .expect("consume"),
            DeleteConfirmation::Stale
        );
    }
}
