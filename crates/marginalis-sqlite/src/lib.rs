//! Marginalisの現行データモデルに限定したSQLite adapter。

mod archive;
mod bibliography_import_repository;
mod bibliography_repository;
mod cleanup;
mod diagnostics;
mod identity_maintenance;
mod math_macro_repository;
mod mcp;
mod mcp_oauth_repository;
mod mcp_scope_ceiling_repository;
mod migration;
mod note_acl;
mod note_graph;
mod note_history;
mod note_repository;
mod note_reviews;
mod note_sync;
mod notes;
mod principal;
mod schema;
mod session;
mod token;
mod webhooks;

pub use cleanup::AuthStatePurgeCounts;
pub use diagnostics::SqliteDiagnosticReport;
pub use identity_maintenance::{
    IdentityMaintenanceError, IdentityMaintenanceReport, IdentityMaintenanceRequest,
};
pub use migration::{DatabaseMigrationError, DatabaseMigrationReport};
pub use session::SqliteOidcLoginAttemptStore;

use crate::schema::initialize_or_validate_schema;
use std::time::Duration;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

#[derive(Clone, Debug)]
pub struct SqliteDatabase {
    pub(crate) pool: SqlitePool,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SqliteStoreError {
    #[error("note was not found or is not accessible")]
    NotFound,
    #[error("note revision does not match")]
    Conflict,
    #[error("archive import target must be empty")]
    ArchiveTargetNotEmpty,
    #[error("stored data is invalid")]
    CorruptData,
    // 内部のSQL失敗内容は利用者へ出さないため、Displayでは伏せる。
    #[error("database query failed")]
    Database(String),
}

/// SQLite内部の失敗を、applicationのrepository port共通の`StorageError`へ写す。
impl From<SqliteStoreError> for marginalis_application::StorageError {
    fn from(error: SqliteStoreError) -> Self {
        match error {
            SqliteStoreError::NotFound => Self::NotFound,
            SqliteStoreError::Conflict => Self::Conflict,
            // アーカイブ復元先が空でないのは保存状態が前提条件を満たさない失敗であり、
            // 再試行では解消しないため`CorruptData`と同じ分類にする。
            SqliteStoreError::ArchiveTargetNotEmpty | SqliteStoreError::CorruptData => {
                Self::CorruptData
            }
            SqliteStoreError::Database(_) => Self::Unavailable,
        }
    }
}

impl SqliteDatabase {
    /// 現行のSQLite schemaへ接続する。
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let validation_options = database_url
            .parse::<SqliteConnectOptions>()?
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let in_memory = is_memory_database_url(database_url);
        let validation_pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(validation_options)
            .await?;
        initialize_or_validate_schema(&validation_pool).await?;
        if in_memory {
            return Ok(Self {
                pool: validation_pool,
            });
        }

        // WALへの切替はdatabase headerを変更する。対応していない旧schemaを検証するだけの
        // 接続ではdatabaseを変更しないよう、schemaの受理後にWAL設定済みのpoolを作る。
        // validation用poolを明示的に閉じ、DELETE modeの接続を運用poolへ残さない。
        validation_pool.close().await;
        let operational_options = database_url
            .parse::<SqliteConnectOptions>()?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(operational_options)
            .await?;
        // WAL接続上でもschemaを再確認し、最初の業務問い合わせより前にWALと共有メモリーを
        // 読み取れる状態へする。最初の検証を通ったdatabaseだけがこの処理へ到達する。
        initialize_or_validate_schema(&pool).await?;
        Ok(Self { pool })
    }

    /// databaseを変更せず、利用可否、schema、整合性を検査する。
    pub async fn diagnose(database_url: &str) -> SqliteDiagnosticReport {
        diagnostics::diagnose(database_url).await
    }

    /// service停止中にSQLite全体を退避し、対応する前進migrationを適用する。
    pub async fn migrate(
        database_url: &str,
        backup_path: &std::path::Path,
    ) -> Result<DatabaseMigrationReport, DatabaseMigrationError> {
        migration::migrate_database(database_url, backup_path).await
    }

    /// service停止中に検証済み退避を作成し、外部identityのbindingを明示的に変更する。
    pub async fn maintain_identity(
        database_url: &str,
        backup_path: &std::path::Path,
        request: IdentityMaintenanceRequest,
    ) -> Result<IdentityMaintenanceReport, IdentityMaintenanceError> {
        identity_maintenance::maintain_identity(database_url, backup_path, request).await
    }
}

fn is_memory_database_url(database_url: &str) -> bool {
    database_url == "sqlite::memory:"
        || database_url
            .split_once('?')
            .is_some_and(|(_, query)| query.split('&').any(|pair| pair == "mode=memory"))
}

pub(crate) fn database_error(error: sqlx::Error) -> SqliteStoreError {
    SqliteStoreError::Database(error.to_string())
}

/// repository実装がsqlxの失敗を`StorageError`へ写すための共通関数。
pub(crate) fn storage_error(error: sqlx::Error) -> marginalis_application::StorageError {
    database_error(error).into()
}

/// SQLiteの`LIKE`で、入力をワイルドカードではなく文字列として含有検索するpattern。
fn like_contains_pattern(value: &str) -> String {
    let mut pattern = String::with_capacity(value.len() + 2);
    pattern.push('%');
    for character in value.to_lowercase().chars() {
        if matches!(character, '!' | '%' | '_') {
            pattern.push('!');
        }
        pattern.push(character);
    }
    pattern.push('%');
    pattern
}

#[cfg(test)]
mod tests;
