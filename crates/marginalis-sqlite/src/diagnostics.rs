//! 運用診断用の読み取り専用SQLite検査。

use serde::Serialize;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{str::FromStr, time::Duration};

use crate::schema::{MIGRATIONS, SCHEMA_VERSION, validate_schema_history};

#[derive(Debug, Serialize)]
pub struct SqliteDiagnosticReport {
    pub available: bool,
    pub schema: DiagnosticCheck<i64>,
    pub integrity: DiagnosticCheck<String>,
    pub foreign_keys: DiagnosticCheck<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<DiagnosticFailure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticCheck<T> {
    pub ok: bool,
    pub actual: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<T>,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticFailure {
    pub check: &'static str,
    pub category: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sqlite_code: Option<i32>,
}

impl SqliteDiagnosticReport {
    pub fn healthy(&self) -> bool {
        self.available && self.schema.ok && self.integrity.ok && self.foreign_keys.ok
    }
}

pub(crate) async fn diagnose(database_url: &str) -> SqliteDiagnosticReport {
    match connect_read_only(database_url).await {
        Ok(pool) => inspect(&pool).await,
        Err(error) => unavailable(&error),
    }
}

async fn connect_read_only(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .read_only(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}

async fn inspect(pool: &SqlitePool) -> SqliteDiagnosticReport {
    let history =
        sqlx::query_scalar::<_, i64>("SELECT version FROM schema_migrations ORDER BY version ASC")
            .fetch_all(pool)
            .await;
    let integrity = sqlx::query("PRAGMA quick_check")
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.try_get::<String, _>(0).ok())
                .collect::<Vec<_>>()
        });
    let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await
        .map(|rows| rows.len() as i64);
    let failures = [
        ("schema", history.as_ref().err()),
        ("integrity", integrity.as_ref().err()),
        ("foreign_keys", foreign_keys.as_ref().err()),
    ]
    .into_iter()
    .filter_map(|(check, error)| error.map(|error| diagnostic_failure(check, error)))
    .collect::<Vec<_>>();
    let summary_error = (!failures.is_empty()).then(|| "query_failed".to_owned());

    SqliteDiagnosticReport {
        available: true,
        schema: DiagnosticCheck {
            ok: history
                .as_ref()
                .is_ok_and(|history| validate_schema_history(history, MIGRATIONS, false).is_ok()),
            actual: history.ok().and_then(|history| history.last().copied()),
            expected: Some(SCHEMA_VERSION),
        },
        integrity: DiagnosticCheck {
            ok: integrity
                .as_ref()
                .is_ok_and(|messages| messages.as_slice() == ["ok"]),
            actual: integrity.ok().map(|messages| messages.join("; ")),
            expected: Some("ok".into()),
        },
        foreign_keys: DiagnosticCheck {
            ok: foreign_keys.as_ref().is_ok_and(|count| *count == 0),
            actual: foreign_keys.ok(),
            expected: Some(0),
        },
        failures,
        error: summary_error,
    }
}

fn unavailable(error: &sqlx::Error) -> SqliteDiagnosticReport {
    SqliteDiagnosticReport {
        available: false,
        schema: DiagnosticCheck {
            ok: false,
            actual: None,
            expected: Some(SCHEMA_VERSION),
        },
        integrity: DiagnosticCheck {
            ok: false,
            actual: None,
            expected: Some("ok".into()),
        },
        foreign_keys: DiagnosticCheck {
            ok: false,
            actual: None,
            expected: Some(0),
        },
        failures: vec![diagnostic_failure("connection", error)],
        error: Some("connection_failed".to_owned()),
    }
}

fn diagnostic_failure(check: &'static str, error: &sqlx::Error) -> DiagnosticFailure {
    let sqlite_code = error
        .as_database_error()
        .and_then(|error| error.code())
        .and_then(|code| code.parse::<i32>().ok());
    let category = match sqlite_code {
        Some(code) => sqlite_result_category(code),
        None => match error {
            sqlx::Error::Io(_) => "io_error",
            sqlx::Error::Configuration(_) => "configuration_error",
            _ => "query_error",
        },
    };
    DiagnosticFailure {
        check,
        category,
        sqlite_code,
    }
}

const fn sqlite_result_category(code: i32) -> &'static str {
    match code & 0xff {
        5 => "database_busy",
        6 => "database_locked",
        8 => "read_only",
        10 => "io_error",
        11 => "corrupt",
        14 => "cannot_open",
        26 => "not_a_database",
        _ => "database_error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SqliteDatabase, schema::initialize_or_validate_schema};
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_directory(purpose: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "marginalis-sqlite-{purpose}-{}-{unique}",
            std::process::id()
        ))
    }

    fn persisted_state(database: &Path) -> Vec<Option<(Vec<u8>, u32)>> {
        ["", "-wal", "-shm"]
            .into_iter()
            .map(|suffix| {
                let path = PathBuf::from(format!("{}{suffix}", database.display()));
                path.exists().then(|| {
                    let metadata = fs::metadata(&path).expect("database file metadata");
                    (
                        fs::read(&path).expect("database file contents"),
                        metadata.permissions().mode(),
                    )
                })
            })
            .collect()
    }

    async fn migrated_database() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("database");
        initialize_or_validate_schema(&pool)
            .await
            .expect("schema migration");
        pool
    }

    #[tokio::test]
    async fn inspect_accepts_the_current_consistent_schema() {
        let report = inspect(&migrated_database().await).await;

        assert!(report.healthy());
        assert_eq!(report.schema.actual, Some(SCHEMA_VERSION));
        assert_eq!(report.integrity.actual.as_deref(), Some("ok"));
        assert_eq!(report.foreign_keys.actual, Some(0));
        assert!(report.failures.is_empty());
        assert_eq!(report.error, None);
    }

    #[tokio::test]
    async fn inspect_reports_schema_and_foreign_key_failures() {
        let pool = migrated_database().await;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&pool)
            .await
            .expect("disable foreign keys for corruption fixture");
        sqlx::query("UPDATE schema_migrations SET version = 1")
            .execute(&pool)
            .await
            .expect("old schema fixture");
        sqlx::query(
            "INSERT INTO mcp_authorization_codes \
             (code_hash, client_id, redirect_uri, redirect_uri_was_supplied, resource_uri, \
              issuer, subject, scopes, code_challenge, expires_at_ms) \
             VALUES (x'00', 'missing-client', 'https://client.example.test/callback', 1, \
              'https://marginalis.example.test/mcp', 'https://id.example.test', 'alice', \
              'notes:read', 'challenge', 1000)",
        )
        .execute(&pool)
        .await
        .expect("foreign key violation fixture");

        let report = inspect(&pool).await;

        assert!(!report.healthy());
        assert!(!report.schema.ok);
        assert_eq!(report.schema.actual, Some(1));
        assert!(!report.foreign_keys.ok);
        assert_eq!(report.foreign_keys.actual, Some(1));
        assert!(report.failures.is_empty());
        assert_eq!(report.error, None);
    }

    #[tokio::test]
    async fn inspect_reports_queries_that_cannot_run() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("database");

        let report = inspect(&pool).await;

        assert!(!report.healthy());
        assert_eq!(report.error.as_deref(), Some("query_failed"));
        assert!(!report.schema.ok);
        assert_eq!(report.integrity.actual.as_deref(), Some("ok"));
        assert_eq!(report.foreign_keys.actual, Some(0));
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].check, "schema");
        assert_eq!(report.failures[0].category, "database_error");
        assert_eq!(report.failures[0].sqlite_code, Some(1));
    }

    #[test]
    fn sqlite_extended_result_codes_have_stable_categories() {
        assert_eq!(sqlite_result_category(261), "database_busy");
        assert_eq!(sqlite_result_category(262), "database_locked");
        assert_eq!(sqlite_result_category(264), "read_only");
        assert_eq!(sqlite_result_category(266), "io_error");
        assert_eq!(sqlite_result_category(267), "corrupt");
        assert_eq!(sqlite_result_category(270), "cannot_open");
        assert_eq!(sqlite_result_category(26), "not_a_database");
        assert_eq!(sqlite_result_category(1), "database_error");
    }

    #[tokio::test]
    async fn rejected_file_schema_remains_unchanged_and_is_repeatedly_diagnosable() {
        let directory = test_directory("old-schema-diagnostics");
        fs::create_dir(&directory).expect("test directory");
        let database = directory.join("marginalis.sqlite");
        let database_url = format!("sqlite://{}?mode=rwc", database.display());
        let fixture = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("fixture database");
        sqlx::query("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY NOT NULL) STRICT")
            .execute(&fixture)
            .await
            .expect("migration table");
        sqlx::query("INSERT INTO schema_migrations (version) VALUES (5)")
            .execute(&fixture)
            .await
            .expect("old schema version");
        fixture.close().await;
        fs::set_permissions(&database, fs::Permissions::from_mode(0o640))
            .expect("database permissions");
        let original = persisted_state(&database);

        let error = SqliteDatabase::connect(&database_url)
            .await
            .expect_err("old schema must be rejected");
        assert!(
            error
                .to_string()
                .contains("unsupported database schema history [5]")
        );
        assert_eq!(persisted_state(&database), original);

        for _ in 0..8 {
            let report = diagnose(&database_url).await;
            assert!(report.available);
            assert!(!report.healthy());
            assert_eq!(report.schema.actual, Some(5));
            assert_eq!(report.integrity.actual.as_deref(), Some("ok"));
            assert_eq!(report.foreign_keys.actual, Some(0));
            assert!(report.failures.is_empty());
            assert_eq!(report.error, None);
            assert_eq!(persisted_state(&database), original);
        }

        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[tokio::test]
    async fn accepted_file_schema_uses_wal_mode() {
        let directory = test_directory("wal-mode");
        fs::create_dir(&directory).expect("test directory");
        let database_path = directory.join("marginalis.sqlite");
        let database_url = format!("sqlite://{}?mode=rwc", database_path.display());

        let database = SqliteDatabase::connect(&database_url)
            .await
            .expect("current schema database");
        let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(&database.pool)
            .await
            .expect("journal mode");

        assert_eq!(journal_mode, "wal");
        database.pool.close().await;
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
