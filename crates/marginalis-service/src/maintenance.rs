//! SQLite正本の保持、移行、backupを行う停止時・定期保守command。

use crate::cli::required_absolute_file_argument;
use marginalis_application::Clock;
use marginalis_asciidoc::validate_archive_notes;
use marginalis_domain::UnixMillis;
use marginalis_server::{StorageConfig, SystemClock};
use marginalis_sqlite::SqliteDatabase;
use std::path::{Path, PathBuf};

const SOFT_DELETE_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

/// 30日間の保持期限を過ぎたソフトデリート済みnoteを物理削除する。
pub(crate) async fn purge_deleted() -> Result<(), Box<dyn std::error::Error>> {
    let configuration = StorageConfig::from_environment()?;
    let database = SqliteDatabase::connect(&configuration.database_url).await?;
    let cutoff = UnixMillis::new(SystemClock.now().get() - SOFT_DELETE_RETENTION_MS);
    let count = database.purge_deleted_before(cutoff).await?;
    tracing::info!(
        count,
        cutoff_ms = cutoff.get(),
        "purged expired soft-deleted notes"
    );
    Ok(())
}

/// SQLite正本をACL・削除状態を含む検証可能なarchiveとして出力する。
pub(crate) async fn export_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = required_absolute_file_argument(&mut arguments, "--output")?;
    if output.exists() {
        return Err(format!("archive output already exists: {}", output.display()).into());
    }
    let configuration = StorageConfig::from_environment()?;
    let archive = SqliteDatabase::connect(&configuration.database_url)
        .await?
        .export_archive()
        .await?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    serde_json::to_writer_pretty(&file, &archive)?;
    file.sync_all()?;
    tracing::info!(output = %output.display(), note_count = archive.notes.len(), "exported archive");
    Ok(())
}

/// archiveを全件検証してから空のSQLite databaseへ一transactionで取り込む。
pub(crate) async fn import_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let file = std::fs::File::open(&input)?;
    let archive = serde_json::from_reader(file)?;
    validate_archive_notes(&archive)?;
    let configuration = StorageConfig::from_environment()?;
    SqliteDatabase::connect(&configuration.database_url)
        .await?
        .import_archive(&archive)
        .await?;
    tracing::info!(input = %input.display(), "imported archive");
    Ok(())
}

/// 停止中のserviceに対してSQLite正本を可搬archiveとして取得する。
pub(crate) async fn backup(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let option = arguments.next();
    let value = arguments.next();
    let output = match (option.as_deref(), value) {
        (Some("--output"), Some(path)) if arguments.next().is_none() => PathBuf::from(path),
        (Some("--directory"), Some(path)) if arguments.next().is_none() => {
            let directory = PathBuf::from(path);
            if !directory.is_absolute() || !directory.is_dir() {
                return Err("backup directory must be an existing absolute directory".into());
            }
            directory.join(format!("backup-{}", SystemClock.now().get()))
        }
        _ => {
            return Err(
                "usage: marginalis backup (--output <absolute-directory>|--directory <absolute-directory>)"
                    .into(),
            );
        }
    };
    if !output.is_absolute() {
        return Err("backup output directory must be an absolute path".into());
    }
    if output.exists() {
        return Err(format!("backup output already exists: {}", output.display()).into());
    }
    std::fs::create_dir(&output)?;

    let result = backup_into(&output).await;
    if let Err(error) = result {
        tracing::error!(output = %output.display(), error = %error, "backup failed; incomplete output was retained");
        return Err(error);
    }
    Ok(())
}

async fn backup_into(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let configuration = StorageConfig::from_environment()?;
    let archive = SqliteDatabase::connect(&configuration.database_url)
        .await?
        .export_archive()
        .await?;
    let archive_path = output.join("marginalis-archive.json");
    let archive_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&archive_path)?;
    serde_json::to_writer_pretty(&archive_file, &archive)?;
    archive_file.sync_all()?;
    std::fs::write(
        output.join("COMPLETE"),
        format!("Marginalis backup {}\n", marginalis_domain::ARCHIVE_FORMAT),
    )?;
    let note_count = archive.notes.len();
    tracing::info!(output = %output.display(), note_count, "backup completed");
    Ok(())
}
