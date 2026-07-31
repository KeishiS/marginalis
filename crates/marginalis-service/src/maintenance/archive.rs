//! Archive commandと復元検証。

use super::{PRIVATE_DIRECTORY_MODE, PRIVATE_FILE_MODE, sync_parent_directory};
use crate::cli::{required_absolute_file_argument, required_archive_migration_arguments};
use crate::config::StorageConfig;
use crate::runtime::SystemClock;
use liblzma::write::XzEncoder;
use marginalis_application::{Clock as _, NoteContent as _};
use marginalis_application::{LogicalSnapshot, RestorePlan};
use marginalis_archive::{
    Archive, create_archive, documents::create_document_export, migrate_previous_archive,
    validate_archive as validate_archive_contract,
};
use marginalis_asciidoc::AsciiDocNoteContent;
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
    let archive = create_archive(&AsciiDocNoteContent, &snapshot);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_MODE)
        .open(&output)?;
    serde_json::to_writer_pretty(&file, &archive)?;
    file.sync_all()?;
    sync_parent_directory(&output)?;
    tracing::info!(event = "maintenance.archive_export.completed", output = %output.display(), note_count = archive.notes.len(), "exported archive");
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
    let migrated = migrate_previous_archive(&AsciiDocNoteContent, &previous)?;
    let snapshot = validate_archive_contract(&AsciiDocNoteContent, &migrated)?;
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
    tracing::info!(event = "maintenance.archive_import.completed", input = %input.display(), "imported archive");
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
    let snapshot = validate_archive_contract(&AsciiDocNoteContent, &archive)?;
    let plan = restore_plan(snapshot)?;
    Ok(ValidatedArchive { archive, plan })
}

pub(super) async fn verify_archive_in_memory(
    validated: &ValidatedArchive,
) -> Result<(), Box<dyn std::error::Error>> {
    let database = SqliteDatabase::connect("sqlite::memory:").await?;
    database.restore(&validated.plan).await?;
    let snapshot = database.export_archive_snapshot().await?;
    if create_archive(&AsciiDocNoteContent, &snapshot) != validated.archive {
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
        let restored = create_archive(&AsciiDocNoteContent, &snapshot);
        if restored != validated.archive {
            return Err("restored archive does not match the source archive".into());
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await;
    if let Err(error) = std::fs::remove_dir_all(&directory) {
        tracing::warn!(
            event = "maintenance.restore_cleanup.failed",
            path = %directory.display(),
            error = %error,
            "failed to remove isolated restore directory"
        );
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

/// SQLite正本を、他の道具で読めるAsciiDocとCSL-JSONのファイル群として出力する。
///
/// 復元の入力はarchiveであり、この出力ではない。取り込み側が移行の要否を判断できるよう、
/// manifestへ形式名と版情報を記録する。
pub(crate) async fn export_documents(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = required_absolute_file_argument(&mut arguments, "--output")?;
    if output.exists() {
        return Err(format!(
            "document export output already exists: {}",
            output.display()
        )
        .into());
    }
    let configuration = StorageConfig::from_environment()?;
    let database = SqliteDatabase::connect(&configuration.database_url).await?;
    let snapshot = database.export_archive_snapshot().await?;
    let export = create_document_export(
        &snapshot,
        env!("CARGO_PKG_VERSION"),
        AsciiDocNoteContent.profile().adocweave_package_version,
        SystemClock.now(),
    );

    let manifest = serde_json::to_vec_pretty(&export.manifest)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_MODE)
        .open(&output)?;
    let mut builder = tar::Builder::new(XzEncoder::new(file, DOCUMENT_EXPORT_COMPRESSION_LEVEL));
    let root = document_export_root(&output)?;
    let modified = u64::try_from(export.manifest.exported_at_ms.max(0) / 1000)?;

    let mut written_directories = HashSet::new();
    for file in export
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.contents.as_bytes()))
        .chain(std::iter::once(("manifest.json", manifest.as_slice())))
    {
        let (path, contents) = file;
        let path = format!("{root}/{path}");
        append_parent_directories(&mut builder, &path, modified, &mut written_directories)?;
        append_private_entry(&mut builder, &path, contents, modified)?;
    }

    let file = builder.into_inner()?.finish()?;
    file.sync_all()?;
    sync_parent_directory(&output)?;

    tracing::info!(
        event = "maintenance.document_export.completed",
        output = %output.display(),
        note_count = export
            .manifest
            .owners
            .iter()
            .map(|owner| owner.notes.len())
            .sum::<usize>(),
        owner_count = export.manifest.owners.len(),
        "exported documents"
    );
    Ok(())
}

/// 圧縮の強さ。既定の6は、取り出しの速さと大きさの釣り合いが取れた値である。
const DOCUMENT_EXPORT_COMPRESSION_LEVEL: u32 = 6;

/// 書庫を展開したときに作られる最上位ディレクトリー名。
///
/// 出力ファイル名から拡張子を除いた名前を使う。展開してもファイルが散らばらず、複数回の
/// 書き出しを並べても混ざらない。
fn document_export_root(output: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("document export output must have a file name")?;
    let stem = name
        .strip_suffix(".tar.xz")
        .or_else(|| name.strip_suffix(".txz"))
        .unwrap_or(name);
    if stem.is_empty() {
        return Err("document export output must have a file name".into());
    }
    Ok(stem.to_owned())
}

/// 書庫内のファイルを、所有者だけが読める権限で加える。
fn append_private_entry(
    builder: &mut tar::Builder<XzEncoder<File>>,
    path: &str,
    contents: &[u8],
    modified: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(PRIVATE_FILE_MODE);
    header.set_mtime(modified);
    header.set_uid(0);
    header.set_gid(0);
    header.set_cksum();
    builder.append_data(&mut header, path, contents)?;
    Ok(())
}

/// 書庫内のディレクトリーを、上位から順に一度だけ加える。
///
/// tarはディレクトリーの項目がなくても展開できるが、その場合の権限は展開する側の設定に
/// 委ねられる。所有者だけが読める状態で展開されるよう、明示的に加える。
fn append_parent_directories(
    builder: &mut tar::Builder<XzEncoder<File>>,
    path: &str,
    modified: u64,
    written: &mut HashSet<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let components = path.split('/').collect::<Vec<_>>();
    let mut directory = String::new();
    for component in &components[..components.len() - 1] {
        if !directory.is_empty() {
            directory.push('/');
        }
        directory.push_str(component);
        if !written.insert(directory.clone()) {
            continue;
        }
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(PRIVATE_DIRECTORY_MODE);
        header.set_mtime(modified);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        builder.append_data(&mut header, format!("{directory}/"), std::io::empty())?;
    }
    Ok(())
}
