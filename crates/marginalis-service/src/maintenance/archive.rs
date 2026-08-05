//! Archive commandと復元検証。

use super::{PRIVATE_DIRECTORY_MODE, PRIVATE_FILE_MODE, PendingOutput};
use crate::cli::{required_absolute_file_argument, required_archive_migration_arguments};
use crate::config::StorageConfig;
use crate::runtime::SystemClock;
use liblzma::read::XzDecoder;
use liblzma::write::XzEncoder;
use marginalis_application::{Clock as _, NoteContent as _};
use marginalis_application::{LogicalSnapshot, RestorePlan};
use marginalis_archive::{
    Archive, create_archive,
    documents::{
        DocumentManifest, archive_from_documents, create_document_export, requires_revalidation,
    },
    migrate_previous_archive, validate_archive as validate_archive_contract,
};
use marginalis_asciidoc::AsciiDocNoteContent;
use marginalis_sqlite::SqliteDatabase;
use std::{
    collections::{BTreeMap, HashSet},
    fs::{DirBuilder, File},
    io::{Read as _, Write as _},
    os::unix::fs::DirBuilderExt,
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
    let (pending, file) = PendingOutput::create(&output)?;
    serde_json::to_writer_pretty(&file, &archive)?;
    file.sync_all()?;
    let written = read_validated_archive(pending.path())?;
    if written.archive != archive {
        return Err("written archive does not match the database snapshot".into());
    }
    pending.commit()?;
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
    let (pending, mut file) = PendingOutput::create(&output)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    let written = read_validated_archive(pending.path())?;
    if written.archive != validated.archive {
        return Err("written migrated archive does not match the validated result".into());
    }
    pending.commit()?;
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
    if !matches_logically(
        &create_archive(&AsciiDocNoteContent, &snapshot),
        &validated.archive,
    ) {
        return Err("archive logical round-trip validation failed".into());
    }
    Ok(())
}

/// 2つのarchiveが同じ内容かどうかを、要素の並びを問わずに判断する。
///
/// archiveの内容は集合であり、並びは組み立て方で変わる。並びの違いで取り込みを止めると、
/// 壊れていない書き出しを拒むことになる。
fn matches_logically(left: &Archive, right: &Archive) -> bool {
    left.clone().canonical() == right.clone().canonical()
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
        if !matches_logically(&restored, &validated.archive) {
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
    let mut citations = HashSet::new();
    for note in snapshot.notes() {
        let analyzed = "validated archive note could not be analyzed";
        for query in
            marginalis_asciidoc::note_reference_queries(note.source()).map_err(|_| analyzed)?
        {
            references.insert((note.note_id(), query.target_note_id));
        }
        for query in
            marginalis_asciidoc::note_citation_queries(note.source()).map_err(|_| analyzed)?
        {
            for key in query.keys {
                citations.insert((note.note_id(), key));
            }
        }
    }
    Ok(RestorePlan::new(
        snapshot,
        references.into_iter().collect(),
        citations.into_iter().collect(),
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
    let (pending, file) = PendingOutput::create(&output)?;
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
    pending.commit()?;

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

/// 展開後の合計の上限。書庫が極端に大きく膨らむ入力を拒否する。
const MAX_DOCUMENT_IMPORT_BYTES: u64 = 1024 * 1024 * 1024;
/// 1項目あたりの上限。
const MAX_DOCUMENT_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

/// 書き出した文書一式を現行規則で再検証し、空のSQLite databaseへ取り込む。
pub(crate) async fn import_documents(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let files = read_document_archive(&input)?;
    let manifest: DocumentManifest = serde_json::from_slice(
        files
            .get("manifest.json")
            .ok_or("document archive has no manifest.json")?,
    )?;
    let package_version = AsciiDocNoteContent.profile().adocweave_package_version;
    let revalidated = requires_revalidation(&manifest, package_version);

    let archive = archive_from_documents(&manifest, &files, package_version, SystemClock.now())?;
    let snapshot = validate_archive_contract(&AsciiDocNoteContent, &archive)?;
    let validated = ValidatedArchive {
        archive,
        plan: restore_plan(snapshot)?,
    };
    verify_archive_in_memory(&validated).await?;

    let configuration = StorageConfig::from_environment()?;
    SqliteDatabase::connect(&configuration.database_url)
        .await?
        .restore(&validated.plan)
        .await?;
    tracing::info!(
        event = "maintenance.document_import.completed",
        input = %input.display(),
        note_count = validated.archive.notes.len(),
        revalidated,
        "imported documents"
    );
    Ok(())
}

/// 書庫を展開し、path構成要素まで検査した内容だけを返す。
///
/// 展開先のファイルシステムへは書かない。`..`や絶対path、symlink、hard linkのように展開先の
/// 外側を指しうる項目と、通常ファイル以外の項目を拒否する。大きさにも上限を設ける。
fn read_document_archive(
    input: &Path,
) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn std::error::Error>> {
    let mut archive = tar::Archive::new(XzDecoder::new(File::open(input)?));
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            return Err("document archive contains an entry that is not a regular file".into());
        }
        let size = entry.header().size()?;
        if size > MAX_DOCUMENT_ENTRY_BYTES {
            return Err("document archive contains an entry that is too large".into());
        }
        total = total
            .checked_add(size)
            .ok_or("document archive size overflows")?;
        if total > MAX_DOCUMENT_IMPORT_BYTES {
            return Err("document archive expands beyond the supported size".into());
        }
        let path = entry.path()?.into_owned();
        let path = safe_archive_path(&path)?;
        let mut contents = Vec::with_capacity(usize::try_from(size)?);
        entry.read_to_end(&mut contents)?;
        if files.insert(path, contents).is_some() {
            return Err("document archive contains a duplicated path".into());
        }
    }
    Ok(files)
}

/// 書庫内のpathを、最上位ディレクトリーを外した相対pathへ直す。
///
/// 絶対pathと`..`、および現在のディレクトリーを指す要素を拒否する。manifestが記録するpathは
/// 最上位ディレクトリーを含まないため、先頭の1要素を外して対応させる。
fn safe_archive_path(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => components.push(
                value
                    .to_str()
                    .ok_or("document archive path is not valid UTF-8")?
                    .to_owned(),
            ),
            _ => return Err("document archive path escapes the archive root".into()),
        }
    }
    if components.len() < 2 {
        return Err("document archive path has no root directory".into());
    }
    Ok(components[1..].join("/"))
}
