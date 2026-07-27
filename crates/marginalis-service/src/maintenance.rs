//! SQLite正本の保持、移行、backupを行う定期・運用保守command。

use crate::cli::required_absolute_file_argument;
use marginalis_application::Clock;
use marginalis_asciidoc::validate_archive_notes;
use marginalis_domain::{Archive, SOFT_DELETE_RETENTION_MS, UnixMillis};
use marginalis_server::{StorageConfig, SystemClock};
use marginalis_sqlite::SqliteDatabase;
use std::{
    cmp::Reverse,
    fs::{DirBuilder, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const PRIVATE_FILE_MODE: u32 = 0o600;
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;

const UNUSED_MCP_CLIENT_RETENTION_MS: i64 = 24 * 60 * 60 * 1_000;

#[derive(serde::Serialize)]
struct DiagnosticReport {
    status: &'static str,
    event: &'static str,
    database: marginalis_sqlite::SqliteDiagnosticReport,
    configuration: PublicConfigurationReport,
}

#[derive(serde::Serialize)]
struct PublicConfigurationReport {
    database_configured: bool,
    base_url: Option<String>,
    listen_address: Option<String>,
    oidc_issuer_url: Option<String>,
    oidc_client_id_configured: bool,
    oidc_ca_certificate_file: Option<String>,
    mcp_enabled: Option<bool>,
    mcp_allowed_origin_count: usize,
}

/// SQLiteと公開設定を変更せずに検査し、結果をJSONで出力する。
pub(crate) async fn diagnose() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = nonempty_environment_variable("MARGINALIS_DATABASE_URL");
    let database = match database_url.as_deref() {
        Some(database_url) => SqliteDatabase::diagnose(database_url).await,
        None => SqliteDatabase::diagnose("sqlite://configuration-is-missing?mode=ro").await,
    };
    let healthy = database.healthy();
    let report = DiagnosticReport {
        status: if healthy { "ok" } else { "failed" },
        event: "diagnostics.completed",
        database,
        configuration: public_configuration(),
    };
    serde_json::to_writer(std::io::stdout().lock(), &report)?;
    println!();
    if healthy {
        Ok(())
    } else {
        Err("diagnostics reported an unhealthy database".into())
    }
}

fn public_configuration() -> PublicConfigurationReport {
    let mcp_allowed_origin_count = std::env::var("MARGINALIS_MCP_ALLOWED_ORIGINS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter(|origin| !origin.trim().is_empty())
                .count()
        })
        .unwrap_or(0);
    PublicConfigurationReport {
        database_configured: nonempty_environment_variable("MARGINALIS_DATABASE_URL").is_some(),
        base_url: nonempty_environment_variable("MARGINALIS_BASE_URL"),
        listen_address: nonempty_environment_variable("MARGINALIS_LISTEN_ADDR"),
        oidc_issuer_url: nonempty_environment_variable("OIDC_ISSUER_URL"),
        oidc_client_id_configured: nonempty_environment_variable("OIDC_CLIENT_ID").is_some(),
        oidc_ca_certificate_file: nonempty_environment_variable("OIDC_CA_CERTIFICATE_FILE"),
        mcp_enabled: std::env::var("MARGINALIS_MCP_ENABLE")
            .ok()
            .and_then(|value| value.parse().ok()),
        mcp_allowed_origin_count,
    }
}

fn nonempty_environment_variable(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// 保持期限を過ぎたnoteと一時的な認証状態を物理削除する。
pub(crate) async fn purge_expired() -> Result<(), Box<dyn std::error::Error>> {
    let result = purge_expired_state().await;
    if let Err(error) = &result {
        tracing::error!(
            event = "maintenance.purge.failed",
            error = %error,
            "failed to purge expired persisted state"
        );
    }
    result
}

async fn purge_expired_state() -> Result<(), Box<dyn std::error::Error>> {
    let configuration = StorageConfig::from_environment()?;
    let database = SqliteDatabase::connect(&configuration.database_url).await?;
    let now = SystemClock.now();
    let note_cutoff = UnixMillis::new(now.get().saturating_sub(SOFT_DELETE_RETENTION_MS));
    let note_count = database.purge_deleted_before(note_cutoff).await?;
    let auth_counts = database
        .purge_expired_auth_state(
            now,
            UnixMillis::new(now.get().saturating_sub(UNUSED_MCP_CLIENT_RETENTION_MS)),
        )
        .await?;
    tracing::info!(
        event = "maintenance.purge.completed",
        note_count,
        web_sessions = auth_counts.web_sessions,
        oidc_login_attempts = auth_counts.oidc_login_attempts,
        mcp_access_tokens = auth_counts.mcp_access_tokens,
        mcp_refresh_tokens = auth_counts.mcp_refresh_tokens,
        mcp_authorization_codes = auth_counts.mcp_authorization_codes,
        mcp_clients = auth_counts.mcp_clients,
        note_cutoff_ms = note_cutoff.get(),
        "purged expired persisted state"
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
        .import_archive(&archive)
        .await?;
    tracing::info!(event = "archive.import.completed", input = %input.display(), "imported archive");
    Ok(())
}

/// archiveのformat、全ノート、ACL、削除状態、revisionを読み取り専用で検証する。
pub(crate) async fn validate_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let archive = read_validated_archive(&input)?;
    verify_archive_in_memory(&archive).await?;
    tracing::info!(input = %input.display(), note_count = archive.notes.len(), "validated archive");
    Ok(())
}

/// archiveを隔離した一時SQLite databaseへ復元し、論理内容の一致を検証する。
pub(crate) async fn verify_restore(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let archive = read_validated_archive(&input)?;
    verify_archive_in_isolated_database(&archive).await?;
    tracing::info!(input = %input.display(), note_count = archive.notes.len(), "verified isolated archive restore");
    Ok(())
}

async fn verify_archive_in_isolated_database(
    archive: &Archive,
) -> Result<(), Box<dyn std::error::Error>> {
    let (directory, database_path) = create_isolated_database_path()?;
    let database_url = format!("sqlite://{}?mode=rwc", database_path.display());
    let result = async {
        let database = SqliteDatabase::connect(&database_url).await?;
        database.import_archive(archive).await?;
        let restored = database.export_archive().await?;
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

/// backupDirectory内の最新成功世代を隔離復元して検証する。
pub(crate) async fn verify_latest_backup(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = required_absolute_file_argument(&mut arguments, "--directory")?;
    let canonical_directory = canonical_backup_directory(&directory)?;
    let generations = validated_successful_generations(&canonical_directory).await?;
    let (_, latest) = generations
        .first()
        .ok_or("no successful backup generation exists")?;
    let archive_path = latest.join("marginalis-archive.json");
    let archive = read_validated_archive(&archive_path)?;
    verify_archive_in_isolated_database(&archive).await?;
    tracing::info!(
        generation = %latest.file_name().and_then(|name| name.to_str()).unwrap_or("<invalid>"),
        note_count = archive.notes.len(),
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
    if !output.is_absolute() {
        return Err("backup output directory must be an absolute path".into());
    }
    if output.exists() {
        return Err(format!("backup output already exists: {}", output.display()).into());
    }
    DirBuilder::new()
        .mode(PRIVATE_DIRECTORY_MODE)
        .create(&output)?;
    sync_parent_directory(&output)?;

    let result = backup_into(&output).await;
    if let Err(error) = result {
        tracing::error!(event = "maintenance.backup.failed", output = %output.display(), error = %error, "backup failed; incomplete output was retained");
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
    use std::io::Write as _;
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
    let canonical_directory = canonical_backup_directory(&directory)?;
    let successful = validated_successful_generations(&canonical_directory).await?;
    let removed_count = successful.len().saturating_sub(keep);
    remove_expired_generations(successful, keep, |path| std::fs::remove_dir_all(path))?;
    File::open(&canonical_directory)?.sync_all()?;
    tracing::info!(directory = %canonical_directory.display(), keep, removed_count, "pruned successful backup generations");
    Ok(())
}

fn remove_expired_generations<E>(
    successful: Vec<(u128, PathBuf)>,
    keep: usize,
    mut remove: impl FnMut(&Path) -> Result<(), E>,
) -> Result<(), E> {
    // 最古の世代から削除し、失敗時にはそれより新しい世代をすべて残す。
    for (_, path) in successful.into_iter().skip(keep).rev() {
        remove(&path)?;
    }
    Ok(())
}

fn canonical_backup_directory(directory: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let canonical = directory.canonicalize()?;
    if !canonical.is_dir() {
        return Err("backup directory is not a directory".into());
    }
    Ok(canonical)
}

async fn validated_successful_generations(
    canonical_directory: &Path,
) -> Result<Vec<(u128, PathBuf)>, Box<dyn std::error::Error>> {
    let mut successful = Vec::new();
    for entry in std::fs::read_dir(canonical_directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(generation) = name.strip_prefix("backup-") else {
            continue;
        };
        let Ok(generation) = generation.parse::<u128>() else {
            continue;
        };
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        let canonical_path = path.canonicalize()?;
        if canonical_path.parent() != Some(canonical_directory) {
            return Err(format!("backup generation escapes backup directory: {name}").into());
        }
        let marker = canonical_path.join("COMPLETE");
        if !marker
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_file())
        {
            continue;
        }
        let expected_marker = format!("Marginalis backup {}\n", marginalis_domain::ARCHIVE_FORMAT);
        if std::fs::read_to_string(&marker)? != expected_marker {
            continue;
        }
        let archive_path = canonical_path.join("marginalis-archive.json");
        if !archive_path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_file())
        {
            continue;
        }
        let archive = read_validated_archive(&archive_path)?;
        verify_archive_in_memory(&archive).await?;
        successful.push((generation, canonical_path));
    }
    successful.sort_by_key(|entry| Reverse(entry.0));
    Ok(successful)
}

fn read_validated_archive(path: &Path) -> Result<Archive, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let archive: Archive = serde_json::from_reader(file)?;
    validate_archive_notes(&archive)?;
    Ok(archive)
}

async fn verify_archive_in_memory(archive: &Archive) -> Result<(), Box<dyn std::error::Error>> {
    let database = SqliteDatabase::connect("sqlite::memory:").await?;
    database.import_archive(archive).await?;
    if database.export_archive().await? != *archive {
        return Err("archive logical round-trip validation failed".into());
    }
    Ok(())
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

fn sync_parent_directory(path: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("output path has no parent directory"))?;
    File::open(parent)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::remove_expired_generations;
    use std::path::{Path, PathBuf};

    #[test]
    fn retention_stops_at_the_first_deletion_failure_and_preserves_newer_generations() {
        let generations = vec![
            (400, PathBuf::from("/backup/backup-400")),
            (300, PathBuf::from("/backup/backup-300")),
            (200, PathBuf::from("/backup/backup-200")),
            (100, PathBuf::from("/backup/backup-100")),
        ];
        let mut attempted = Vec::new();

        let result = remove_expired_generations(generations, 1, |path: &Path| {
            attempted.push(path.to_path_buf());
            if path.ends_with("backup-200") {
                Err("simulated deletion failure")
            } else {
                Ok(())
            }
        });

        assert_eq!(result, Err("simulated deletion failure"));
        assert_eq!(
            attempted,
            [
                PathBuf::from("/backup/backup-100"),
                PathBuf::from("/backup/backup-200")
            ]
        );
    }
}
