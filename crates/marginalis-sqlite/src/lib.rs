//! Marginalisの現行データモデルに限定したSQLite adapter。

mod archive;
mod bibliography_repository;
mod cleanup;
mod diagnostics;
mod note_repository;
mod notes;
mod schema;
mod session;
mod token;
mod web_session_repository;

pub use cleanup::AuthStatePurgeCounts;
pub use diagnostics::SqliteDiagnosticReport;
pub use session::SqliteOidcLoginAttemptStore;

use crate::schema::initialize_or_validate_schema;
use std::{fmt, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

#[derive(Clone, Debug)]
pub struct SqliteDatabase {
    pub(crate) pool: SqlitePool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SqliteStoreError {
    NotFound,
    Conflict,
    ArchiveTargetNotEmpty,
    CorruptData,
    Database(String),
}

impl fmt::Display for SqliteStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("note was not found or is not accessible"),
            Self::Conflict => formatter.write_str("note revision does not match"),
            Self::ArchiveTargetNotEmpty => {
                formatter.write_str("archive import target must be empty")
            }
            Self::CorruptData => formatter.write_str("stored data is invalid"),
            Self::Database(_) => formatter.write_str("database query failed"),
        }
    }
}

impl std::error::Error for SqliteStoreError {}

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

#[cfg(test)]
mod tests;
