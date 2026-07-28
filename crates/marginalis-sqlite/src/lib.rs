//! Marginalisの現行データモデルに限定したSQLite adapter。

mod archive;
mod cleanup;
mod diagnostics;
mod mcp;
mod note_repository;
mod notes;
mod schema;
mod session;
mod token;

pub use cleanup::AuthStatePurgeCounts;
pub use diagnostics::SqliteDiagnosticReport;
pub use session::SqliteOidcLoginAttemptStore;

use crate::schema::migrate;
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

    /// databaseを変更せず、利用可否、schema、整合性を検査する。
    pub async fn diagnose(database_url: &str) -> SqliteDiagnosticReport {
        diagnostics::diagnose(database_url).await
    }
}

pub(crate) fn database_error(error: sqlx::Error) -> SqliteStoreError {
    SqliteStoreError::Database(error.to_string())
}

#[cfg(test)]
mod tests;
