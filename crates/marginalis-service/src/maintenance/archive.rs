//! Archive commandと復元検証。

use super::{PRIVATE_DIRECTORY_MODE, PRIVATE_FILE_MODE, sync_parent_directory};
use crate::cli::{required_absolute_file_argument, required_archive_migration_arguments};
use crate::config::StorageConfig;
use marginalis_application::{LogicalSnapshot, RestorePlan};
use marginalis_asciidoc::{
    Archive, create_archive, migrate_previous_archive,
    validate_archive as validate_archive_contract,
};
use marginalis_sqlite::SqliteDatabase;
use std::{
    collections::HashSet,
    fs::{DirBuilder, File, OpenOptions},
    io::Write as _,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// SQLite正本を所有者・削除状態を含む検証可能なarchiveとして出力する。
pub(crate) async fn export_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = required_absolute_file_argument(&mut arguments, "--output")?;
    if output.exists() {
        return Err(format!("archive output already exists: {}", output.display()).into());
    }
    let configuration = StorageConfig::from_environment()?;
    let database = SqliteDatabase::connect(&configuration.database_url).await?;
    let snapshot = database.export_archive_snapshot().await?;
    let archive = create_archive(&snapshot);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_MODE)
        .open(&output)?;
    serde_json::to_writer_pretty(&file, &archive)?;
    file.sync_all()?;
    sync_parent_directory(&output)?;
    tracing::info!(event = "archive.export.completed", output = %output.display(), note_count = archive.notes.len(), "exported archive");
    Ok(())
}

/// 直前のarchiveを現行の文書規則で全件再検証し、新しいファイルへ変換する。
pub(crate) async fn migrate_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (input, output) = required_archive_migration_arguments(&mut arguments)?;
    if output.exists() {
        return Err(format!(
            "archive migration output already exists: {}",
            output.display()
        )
        .into());
    }
    let previous: Archive = serde_json::from_reader(File::open(&input)?)?;
    let migrated = migrate_previous_archive(&previous)?;
    let snapshot = validate_archive_contract(&migrated)?;
    let validated = ValidatedArchive {
        archive: migrated,
        plan: restore_plan(snapshot)?,
    };
    verify_archive_in_memory(&validated).await?;

    let encoded = serde_json::to_vec_pretty(&validated.archive)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_MODE)
        .open(&output)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    let written = read_validated_archive(&output)?;
    if written.archive != validated.archive {
        return Err("written migrated archive does not match the validated result".into());
    }
    sync_parent_directory(&output)?;
    tracing::info!(
        event = "maintenance.archive_migration.completed",
        input = %input.display(),
        output = %output.display(),
        note_count = written.archive.notes.len(),
        "migrated archive"
    );
    Ok(())
}

/// archiveを全件検証してから空のSQLite databaseへ一transactionで取り込む。
pub(crate) async fn import_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let validated = read_validated_archive(&input)?;
    let configuration = StorageConfig::from_environment()?;
    SqliteDatabase::connect(&configuration.database_url)
        .await?
        .restore(&validated.plan)
        .await?;
    tracing::info!(event = "archive.import.completed", input = %input.display(), "imported archive");
    Ok(())
}

/// archiveのformat、全ノート、所有者、削除状態、revisionを読み取り専用で検証する。
pub(crate) async fn validate_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let validated = read_validated_archive(&input)?;
    verify_archive_in_memory(&validated).await?;
    tracing::info!(
        event = "maintenance.archive_validation.completed",
        input = %input.display(),
        note_count = validated.archive.notes.len(),
        "validated archive"
    );
    Ok(())
}

/// archiveを隔離した一時SQLite databaseへ復元し、論理内容の一致を検証する。
pub(crate) async fn verify_restore(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let validated = read_validated_archive(&input)?;
    verify_archive_in_isolated_database(&validated).await?;
    tracing::info!(
        event = "maintenance.restore_verification.completed",
        input = %input.display(),
        note_count = validated.archive.notes.len(),
        "verified isolated archive restore"
    );
    Ok(())
}

pub(super) struct ValidatedArchive {
    pub(super) archive: Archive,
    pub(super) plan: RestorePlan,
}

pub(super) fn read_validated_archive(
    path: &Path,
) -> Result<ValidatedArchive, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let archive: Archive = serde_json::from_reader(file)?;
    let snapshot = validate_archive_contract(&archive)?;
    let plan = restore_plan(snapshot)?;
    Ok(ValidatedArchive { archive, plan })
}

pub(super) async fn verify_archive_in_memory(
    validated: &ValidatedArchive,
) -> Result<(), Box<dyn std::error::Error>> {
    let database = SqliteDatabase::connect("sqlite::memory:").await?;
    database.restore(&validated.plan).await?;
    let snapshot = database.export_archive_snapshot().await?;
    if create_archive(&snapshot) != validated.archive {
        return Err("archive logical round-trip validation failed".into());
    }
    Ok(())
}

pub(super) async fn verify_archive_in_isolated_database(
    validated: &ValidatedArchive,
) -> Result<(), Box<dyn std::error::Error>> {
    let (directory, database_path) = create_isolated_database_path()?;
    let database_url = format!("sqlite://{}?mode=rwc", database_path.display());
    let result = async {
        let database = SqliteDatabase::connect(&database_url).await?;
        database.restore(&validated.plan).await?;
        let snapshot = database.export_archive_snapshot().await?;
        let restored = create_archive(&snapshot);
        if restored != validated.archive {
            return Err("restored archive does not match the source archive".into());
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await;
    if let Err(error) = std::fs::remove_dir_all(&directory) {
        tracing::warn!(path = %directory.display(), error = %error, "failed to remove isolated restore directory");
    }
    result
}

fn restore_plan(snapshot: LogicalSnapshot) -> Result<RestorePlan, Box<dyn std::error::Error>> {
    let mut references = HashSet::new();
    for note in snapshot.notes() {
        for query in marginalis_asciidoc::note_reference_queries(note.source())
            .map_err(|_| "validated archive note could not be analyzed")?
        {
            references.insert((note.note_id(), query.target_note_id));
        }
    }
    Ok(RestorePlan::new(
        snapshot,
        references.into_iter().collect(),
    )?)
}

fn create_isolated_database_path() -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let base = std::env::temp_dir();
    let seed = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    for attempt in 0_u8..16 {
        let directory = base.join(format!(
            "marginalis-restore-check-{}-{seed}-{attempt}",
            std::process::id()
        ));
        match DirBuilder::new()
            .mode(PRIVATE_DIRECTORY_MODE)
            .create(&directory)
        {
            Ok(()) => {
                let database_path = directory.join("restored.sqlite");
                return Ok((directory, database_path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err("could not allocate an isolated restore directory".into())
}
