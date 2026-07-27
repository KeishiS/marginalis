//! Backup世代の耐久的な生成。

use super::super::{
    PRIVATE_DIRECTORY_MODE, PRIVATE_FILE_MODE,
    archive::{read_validated_archive, verify_archive_in_memory},
    sync_parent_directory,
};
use marginalis_server::StorageConfig;
use marginalis_sqlite::SqliteDatabase;
use std::{
    fs::{DirBuilder, File, OpenOptions},
    io::Write as _,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
};

pub(super) async fn create(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !output.is_absolute() {
        return Err("backup output directory must be an absolute path".into());
    }
    if output.exists() {
        return Err(format!("backup output already exists: {}", output.display()).into());
    }
    DirBuilder::new()
        .mode(PRIVATE_DIRECTORY_MODE)
        .create(output)?;
    sync_parent_directory(output)?;

    let result = populate(output).await;
    if let Err(error) = result {
        tracing::error!(event = "maintenance.backup.failed", output = %output.display(), error = %error, "backup failed; incomplete output was retained");
        return Err(error);
    }
    Ok(())
}

async fn populate(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let configuration = StorageConfig::from_environment()?;
    let notes = SqliteDatabase::connect(&configuration.database_url)
        .await?
        .export_notes()
        .await?;
    let archive = marginalis_asciidoc::create_archive(notes);
    let archive_path = output.join("marginalis-archive.json");
    let archive_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_MODE)
        .open(&archive_path)?;
    serde_json::to_writer_pretty(&archive_file, &archive)?;
    archive_file.sync_all()?;
    let written_archive = read_validated_archive(&archive_path)?;
    if written_archive != archive {
        return Err("written backup archive does not match the database snapshot".into());
    }
    verify_archive_in_memory(&written_archive).await?;
    let marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_MODE)
        .open(output.join("COMPLETE"))?;
    writeln!(
        &marker,
        "Marginalis backup {}",
        marginalis_domain::ARCHIVE_FORMAT
    )?;
    marker.sync_all()?;
    File::open(output)?.sync_all()?;
    let note_count = archive.notes.len();
    tracing::info!(event = "maintenance.backup.completed", output = %output.display(), note_count, "backup completed");
    Ok(())
}
