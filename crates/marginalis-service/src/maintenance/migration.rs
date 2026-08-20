//! SQLite schemaを明示的に前進移行する保守command。

use crate::{config::StorageConfig, runtime::SystemClock};
use marginalis_application::Clock;
use marginalis_sqlite::SqliteDatabase;
use std::path::PathBuf;

pub(crate) async fn migrate_database(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let option = arguments.next();
    let value = arguments.next();
    let backup_path = match (option.as_deref(), value) {
        (Some("--output"), Some(path)) if arguments.next().is_none() => {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err("database migration output must be an absolute file path".into());
            }
            path
        }
        (Some("--directory"), Some(path)) if arguments.next().is_none() => {
            let directory = PathBuf::from(path);
            if !directory.is_absolute() || !directory.is_dir() {
                return Err(
                    "database migration directory must be an existing absolute directory".into(),
                );
            }
            directory.join(format!(
                "database-migration-{}.sqlite3",
                SystemClock.now().get()
            ))
        }
        _ => {
            return Err(
                "usage: marginalis migrate-database (--output <absolute-file>|--directory <absolute-directory>)"
                    .into(),
            );
        }
    };

    let configuration = StorageConfig::from_environment()?;
    let report = SqliteDatabase::migrate(&configuration.database_url, &backup_path).await?;
    let published_backup = report
        .backup_path
        .as_deref()
        .map(|path| path.display().to_string());
    tracing::info!(
        event = "maintenance.database_migration.completed",
        from_schema = report.from_version,
        to_schema = report.to_version,
        applied_migrations = report.applied_migrations,
        backup = published_backup.as_deref().unwrap_or("none"),
        "database migration completed"
    );
    Ok(())
}
