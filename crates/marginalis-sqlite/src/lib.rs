//! MarginalisのSQLite adapterと、version管理されたschema migration。

use std::time::Duration;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("../migrations/0001_initial.sql"))];

#[derive(Clone, Debug)]
pub struct SqliteDatabase {
    pool: SqlitePool,
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
}
