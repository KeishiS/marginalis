//! 現行profile専用の破壊的初期化とschema version検証。

use sqlx::SqlitePool;

pub(crate) const SCHEMA_VERSION: i64 = 9;
const INITIAL_SCHEMA: &str = include_str!("schema.sql");

pub(crate) async fn initialize_or_validate_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY NOT NULL) STRICT",
    )
    .execute(&mut *transaction)
    .await?;
    let version =
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(version), 0) FROM schema_migrations")
            .fetch_one(&mut *transaction)
            .await?;
    if version == 0 {
        let existing_tables = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'table'
               AND name NOT LIKE 'sqlite_%'
               AND name != 'schema_migrations'",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if existing_tables != 0 {
            return Err(sqlx::Error::Protocol(
                "database initialization requires an empty database".into(),
            ));
        }
        sqlx::raw_sql(INITIAL_SCHEMA)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("INSERT INTO schema_migrations (version) VALUES (?)")
            .bind(SCHEMA_VERSION)
            .execute(&mut *transaction)
            .await?;
    } else if version != SCHEMA_VERSION {
        return Err(sqlx::Error::Protocol(format!(
            "unsupported database schema version {version}; expected {SCHEMA_VERSION}"
        )));
    }
    transaction.commit().await
}
