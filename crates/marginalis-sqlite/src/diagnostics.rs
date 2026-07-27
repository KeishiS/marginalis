//! 運用診断用の読み取り専用SQLite検査。

use serde::Serialize;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{str::FromStr, time::Duration};

use crate::schema::SCHEMA_VERSION;

#[derive(Debug, Serialize)]
pub struct SqliteDiagnosticReport {
    pub available: bool,
    pub schema: DiagnosticCheck<i64>,
    pub integrity: DiagnosticCheck<String>,
    pub foreign_keys: DiagnosticCheck<i64>,
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

impl SqliteDiagnosticReport {
    pub fn healthy(&self) -> bool {
        self.available && self.schema.ok && self.integrity.ok && self.foreign_keys.ok
    }
}

pub(crate) async fn diagnose(database_url: &str) -> SqliteDiagnosticReport {
    match connect_read_only(database_url).await {
        Ok(pool) => inspect(&pool).await,
        Err(_) => unavailable("connection_failed"),
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
    let version =
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(version), 0) FROM schema_migrations")
            .fetch_one(pool)
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
    let first_error = (version.is_err() || integrity.is_err() || foreign_keys.is_err())
        .then(|| "query_failed".to_owned());

    SqliteDiagnosticReport {
        available: true,
        schema: DiagnosticCheck {
            ok: version.as_ref().is_ok_and(|value| *value == SCHEMA_VERSION),
            actual: version.ok(),
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
        error: first_error,
    }
}

fn unavailable(error: &str) -> SqliteDiagnosticReport {
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
        error: Some(error.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::migrate;

    async fn migrated_database() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("database");
        migrate(&pool).await.expect("schema migration");
        pool
    }

    #[tokio::test]
    async fn inspect_accepts_the_current_consistent_schema() {
        let report = inspect(&migrated_database().await).await;

        assert!(report.healthy());
        assert_eq!(report.schema.actual, Some(SCHEMA_VERSION));
        assert_eq!(report.integrity.actual.as_deref(), Some("ok"));
        assert_eq!(report.foreign_keys.actual, Some(0));
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
             (code_hash, client_id, redirect_uri, resource_uri, issuer, subject, \
              is_administrator, scopes, code_challenge, expires_at_ms) \
             VALUES (x'00', 'missing-client', 'https://client.example.test/callback', \
              'https://marginalis.example.test/mcp', 'https://id.example.test', 'alice', \
              0, 'notes:read', 'challenge', 1000)",
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
    }
}
