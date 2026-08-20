//! 現行schemaの初期化と、前進migrationに使う履歴検証。

use sqlx::SqlitePool;

pub(crate) const BASELINE_SCHEMA_VERSION: i64 = 22;
pub(crate) const SCHEMA_VERSION: i64 = 22;
const INITIAL_SCHEMA: &str = include_str!("schema.sql");

/// 公開済みschema間の前進migration。
///
/// 各要素は前の``to``から次の``from``へ途切れず、1版ずつ進む。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Migration {
    pub(crate) from: i64,
    pub(crate) to: i64,
    pub(crate) sql: &'static str,
}

// schema 22は前進migrationの基準。最初の変更は#549で22から23へ進める。
pub(crate) const MIGRATIONS: &[Migration] = &[];

pub(crate) fn expected_schema_history(migrations: &[Migration]) -> Result<Vec<i64>, sqlx::Error> {
    expected_schema_history_for(migrations, SCHEMA_VERSION)
}

pub(crate) fn expected_schema_history_for(
    migrations: &[Migration],
    current_version: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    let mut expected = vec![BASELINE_SCHEMA_VERSION];
    let mut previous = BASELINE_SCHEMA_VERSION;
    for migration in migrations {
        if migration.from != previous || migration.to != migration.from + 1 {
            return Err(sqlx::Error::Protocol(
                "database migration plan is not consecutive".into(),
            ));
        }
        expected.push(migration.to);
        previous = migration.to;
    }
    if previous != current_version {
        return Err(sqlx::Error::Protocol(format!(
            "database migration plan ends at {previous}; expected {current_version}"
        )));
    }
    Ok(expected)
}

pub(crate) fn validate_schema_history(
    history: &[i64],
    migrations: &[Migration],
    allow_pending: bool,
) -> Result<usize, sqlx::Error> {
    validate_schema_history_for(history, migrations, SCHEMA_VERSION, allow_pending)
}

pub(crate) fn validate_schema_history_for(
    history: &[i64],
    migrations: &[Migration],
    current_version: i64,
    allow_pending: bool,
) -> Result<usize, sqlx::Error> {
    let expected = expected_schema_history_for(migrations, current_version)?;
    let is_supported_prefix = !history.is_empty()
        && history.len() <= expected.len()
        && history == &expected[..history.len()];
    if !is_supported_prefix {
        return Err(sqlx::Error::Protocol(format!(
            "unsupported database schema history {history:?}; expected {expected:?}"
        )));
    }
    let applied = history.len() - 1;
    if !allow_pending && applied != migrations.len() {
        return Err(sqlx::Error::Protocol(format!(
            "database schema history {history:?} requires migrate-database; expected {expected:?}"
        )));
    }
    Ok(applied)
}

pub(crate) async fn initialize_or_validate_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY NOT NULL) STRICT",
    )
    .execute(&mut *transaction)
    .await?;
    let history =
        sqlx::query_scalar::<_, i64>("SELECT version FROM schema_migrations ORDER BY version ASC")
            .fetch_all(&mut *transaction)
            .await?;
    if history.is_empty() {
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
        for version in expected_schema_history(MIGRATIONS)? {
            sqlx::query("INSERT INTO schema_migrations (version) VALUES (?)")
                .bind(version)
                .execute(&mut *transaction)
                .await?;
        }
    } else {
        validate_schema_history(&history, MIGRATIONS, false)?;
    }
    transaction.commit().await
}
