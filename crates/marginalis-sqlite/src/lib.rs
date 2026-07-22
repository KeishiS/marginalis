//! MarginalisのSQLite adapterと、version管理されたschema migration。

use std::{fmt, future::Future, str::FromStr, time::Duration};

use marginalis_application::{
    JournalEntry, NoteOperationKind, OidcIdentityStore, OperationId, OperationJournal,
    OperationState,
};
use marginalis_domain::{
    EntityId, NoteId, OidcIdentity, OidcLoginResult, OidcUser, RegistrationPolicy, SourceRevision,
    UnixMillis, UserId, UserStatus,
};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("../migrations/0001_initial.sql"))];

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
                 (operation_id, kind, state, note_id, source_revision, created_at_ms, updated_at_ms)
                 VALUES (?, ?, 'prepared', ?, ?, ?, ?)",
            )
            .bind(entry.operation_id.0.to_string())
            .bind(operation_kind(entry.kind))
            .bind(entry.note_id.to_string())
            .bind(
                entry
                    .source_revision
                    .map(|revision| revision.bytes().to_vec()),
            )
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
                "SELECT operation_id, kind, state, note_id, source_revision, created_at_ms, updated_at_ms
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
    Ok(JournalEntry {
        operation_id: OperationId(
            EntityId::from_str(&operation_id).map_err(|_| JournalError::CorruptEntry)?,
        ),
        note_id: NoteId::new(EntityId::from_str(&note_id).map_err(|_| JournalError::CorruptEntry)?),
        kind,
        state,
        source_revision,
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
        JournalEntry, NoteOperationKind, OidcIdentityStore, OperationId, OperationJournal,
        OperationState,
    };
    use marginalis_domain::{
        EntityId, NoteId, OidcIdentity, OidcLoginResult, RegistrationPolicy, UnixMillis, UserId,
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
}
