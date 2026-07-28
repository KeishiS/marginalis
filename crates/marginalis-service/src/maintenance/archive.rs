//! Archive commandと復元検証。

use super::{PRIVATE_DIRECTORY_MODE, PRIVATE_FILE_MODE, sync_parent_directory};
use crate::cli::required_absolute_file_argument;
use marginalis_asciidoc::{create_archive, validate_archive as validate_archive_contract};
use marginalis_domain::Archive;
use marginalis_server::StorageConfig;
use marginalis_sqlite::SqliteDatabase;
use std::{
    collections::HashSet,
    fs::{DirBuilder, File, OpenOptions},
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
    let (notes, note_acl) = database.export_archive_snapshot().await?;
    let archive = create_archive(notes, note_acl);
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

/// archiveを全件検証してから空のSQLite databaseへ一transactionで取り込む。
pub(crate) async fn import_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let archive = read_validated_archive(&input)?;
    let configuration = StorageConfig::from_environment()?;
    SqliteDatabase::connect(&configuration.database_url)
        .await?
        .import_notes(
            &archive.notes,
            &archive_references(&archive)?,
            &archive.note_acl,
        )
        .await?;
    tracing::info!(event = "archive.import.completed", input = %input.display(), "imported archive");
    Ok(())
}

/// archiveのformat、全ノート、所有者、削除状態、revisionを読み取り専用で検証する。
pub(crate) async fn validate_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let archive = read_validated_archive(&input)?;
    verify_archive_in_memory(&archive).await?;
    tracing::info!(
        event = "maintenance.archive_validation.completed",
        input = %input.display(),
        note_count = archive.notes.len(),
        "validated archive"
    );
    Ok(())
}

/// archiveを隔離した一時SQLite databaseへ復元し、論理内容の一致を検証する。
pub(crate) async fn verify_restore(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let archive = read_validated_archive(&input)?;
    verify_archive_in_isolated_database(&archive).await?;
    tracing::info!(
        event = "maintenance.restore_verification.completed",
        input = %input.display(),
        note_count = archive.notes.len(),
        "verified isolated archive restore"
    );
    Ok(())
}

pub(super) fn read_validated_archive(path: &Path) -> Result<Archive, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let archive: Archive = serde_json::from_reader(file)?;
    validate_archive_contract(&archive)?;
    Ok(archive)
}

pub(super) async fn verify_archive_in_memory(
    archive: &Archive,
) -> Result<(), Box<dyn std::error::Error>> {
    let database = SqliteDatabase::connect("sqlite::memory:").await?;
    database
        .import_notes(
            &archive.notes,
            &archive_references(archive)?,
            &archive.note_acl,
        )
        .await?;
    let (notes, note_acl) = database.export_archive_snapshot().await?;
    if create_archive(notes, note_acl) != *archive {
        return Err("archive logical round-trip validation failed".into());
    }
    Ok(())
}

pub(super) async fn verify_archive_in_isolated_database(
    archive: &Archive,
) -> Result<(), Box<dyn std::error::Error>> {
    let (directory, database_path) = create_isolated_database_path()?;
    let database_url = format!("sqlite://{}?mode=rwc", database_path.display());
    let result = async {
        let database = SqliteDatabase::connect(&database_url).await?;
        database
            .import_notes(
                &archive.notes,
                &archive_references(archive)?,
                &archive.note_acl,
            )
            .await?;
        let (notes, note_acl) = database.export_archive_snapshot().await?;
        let restored = create_archive(notes, note_acl);
        if restored != *archive {
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

fn archive_references(
    archive: &Archive,
) -> Result<Vec<(marginalis_domain::NoteId, marginalis_domain::NoteId)>, Box<dyn std::error::Error>>
{
    let mut references = HashSet::new();
    for note in &archive.notes {
        for query in marginalis_asciidoc::note_reference_queries(note)
            .map_err(|_| "validated archive note could not be analyzed")?
        {
            references.insert((note.note_id, query.target_note_id));
        }
    }
    Ok(references.into_iter().collect())
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
