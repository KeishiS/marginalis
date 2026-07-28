//! Backup commandの引数処理と処理順序。

mod generation;
mod repository;

use super::archive::{read_validated_archive, verify_archive_in_isolated_database};
use crate::cli::required_absolute_file_argument;
use crate::runtime::SystemClock;
use marginalis_application::Clock;
use std::{fs::File, path::PathBuf};

/// backupDirectory内の最新成功世代を隔離復元して検証する。
pub(crate) async fn verify_latest_backup(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = required_absolute_file_argument(&mut arguments, "--directory")?;
    let canonical_directory = repository::canonical_directory(&directory)?;
    let generations = repository::validated_successful_generations(&canonical_directory).await?;
    let (_, latest) = generations
        .first()
        .ok_or("no successful backup generation exists")?;
    let archive_path = latest.join("marginalis-archive.json");
    let archive = read_validated_archive(&archive_path)?;
    verify_archive_in_isolated_database(&archive).await?;
    tracing::info!(
        event = "maintenance.backup_verification.completed",
        generation = %latest.file_name().and_then(|name| name.to_str()).unwrap_or("<invalid>"),
        note_count = archive.archive.notes.len(),
        "verified latest backup generation"
    );
    Ok(())
}

/// SQLiteの一貫したsnapshotを可搬archiveとして取得する。
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
    generation::create(&output).await
}

/// backupDirectory直下の検証済み成功世代だけを新しい順に保持する。
pub(crate) async fn prune_backups(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory_option = arguments.next();
    let directory_value = arguments.next();
    let keep_option = arguments.next();
    let keep_value = arguments.next();
    if directory_option.as_deref() != Some("--directory")
        || keep_option.as_deref() != Some("--keep")
        || arguments.next().is_some()
    {
        return Err(
            "usage: marginalis prune-backups --directory <absolute-directory> --keep <positive-count>"
                .into(),
        );
    }
    let directory = PathBuf::from(directory_value.ok_or("backup directory is required")?);
    let keep: usize = keep_value
        .ok_or("backup retention count is required")?
        .parse()?;
    if !directory.is_absolute() || keep == 0 {
        return Err("backup directory must be absolute and keep must be positive".into());
    }

    let canonical_directory = repository::canonical_directory(&directory)?;
    let successful = repository::validated_successful_generations(&canonical_directory).await?;
    let removed_count = successful.len().saturating_sub(keep);
    repository::remove_expired_generations(successful, keep, |path| std::fs::remove_dir_all(path))?;
    File::open(&canonical_directory)?.sync_all()?;
    tracing::info!(
        event = "maintenance.backup_prune.completed",
        directory = %canonical_directory.display(),
        keep,
        removed_count,
        "pruned successful backup generations"
    );
    Ok(())
}
