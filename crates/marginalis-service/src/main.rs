//! Marginalisのcomposition root。設定読込、adapter組立、tracingおよびHTTP listenを担う。

use marginalis_application::{Clock, RootCredentialStore, RootInitializationService};
use marginalis_asciidoc::parse_note_projection;
use marginalis_auth_oidc::{OidcAuthentication, OidcConfiguration};
use marginalis_domain::UnixMillis;
use marginalis_files::{FileNoteStore, StorageLayout};
use marginalis_server::{
    ServerConfig, ServerMcpAuthenticator, ServerMcpOAuthService, ServerNoteUseCases,
    ServerWebAuthenticationUseCases, StorageConfig, SystemClock, SystemRandom,
};
use marginalis_sqlite::SqliteDatabase;
use marginalis_web::{ApiState, McpEndpoint, router};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    initialize_tracing();
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next();
    let result = match command.as_deref() {
        None | Some("serve") => run().await,
        Some("rebuild-projections") => rebuild_projections().await,
        Some("prune-audit") => prune_audit().await,
        Some("backup") => backup(arguments).await,
        Some("restore") => restore(arguments).await,
        Some(_) => Err(
            "usage: marginalis [serve|rebuild-projections|prune-audit|backup (--output <absolute-directory>|--directory <absolute-directory>)|restore --input <backup-directory> --output <new-data-directory>]"
                .into(),
        ),
    };
    if let Err(error) = result {
        tracing::error!(error = %error, "Marginalis server terminated");
        std::process::exit(1);
    }
}

/// 停止中のserviceに対してSQLiteとAsciiDoc正本を一組で取得するbackup command。
async fn backup(
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
    let layout = StorageLayout::open(&configuration.data_dir)?;
    let database = SqliteDatabase::connect_with_initial_registration_policy(
        &configuration.database_url,
        configuration.initial_registration_policy,
    )
    .await?;
    let sources = FileNoteStore::open(layout.data_directory())?;
    let database_path = output.join("marginalis.sqlite");
    let database_path = database_path
        .to_str()
        .ok_or("backup output directory must be valid UTF-8")?;
    database.backup_to(database_path).await?;
    let note_count = sources.copy_sources_to(output)?;
    layout.copy_format_to(output)?;
    write_backup_manifest(output, SystemClock.now())?;
    std::fs::write(output.join("COMPLETE"), "Marginalis backup format 1\n")?;
    tracing::info!(output = %output.display(), note_count, "backup completed");
    Ok(())
}

/// 完了済みbackupを検証し、既存dataDirを変更せずに新しいdataDir候補を作成する。
///
/// 実際にどのdataDirへ切り替えるかは運用者の判断に委ねる。これにより復元元・現行正本を
/// 自動削除せず、NixOS設定切替前に内容を確認できる。
async fn restore(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = arguments.next();
    let backup = arguments.next();
    let output = arguments.next();
    let data_directory = arguments.next();
    if input.as_deref() != Some("--input")
        || output.as_deref() != Some("--output")
        || arguments.next().is_some()
    {
        return Err(
            "usage: marginalis restore --input <backup-directory> --output <new-data-directory>"
                .into(),
        );
    }
    let backup = PathBuf::from(backup.ok_or("missing backup directory")?);
    let data_directory = PathBuf::from(data_directory.ok_or("missing output data directory")?);
    if !backup.is_absolute() || !data_directory.is_absolute() {
        return Err("restore paths must be absolute".into());
    }
    if data_directory.exists() {
        return Err(format!(
            "restore output already exists: {}",
            data_directory.display()
        )
        .into());
    }
    restore_into(&backup, &data_directory).await
}

async fn restore_into(
    backup: &Path,
    data_directory: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    StorageLayout::validate_existing(backup)?;
    let marker = std::fs::read_to_string(backup.join("COMPLETE"))?;
    if marker != "Marginalis backup format 1\n" {
        return Err("backup COMPLETE marker is missing or unsupported".into());
    }
    let database = backup.join("marginalis.sqlite");
    if !database.is_file() || !backup.join("notes").is_dir() {
        return Err(
            "backup does not contain the required SQLite database and notes directory".into(),
        );
    }
    verify_backup_manifest(backup)?;
    SqliteDatabase::validate_backup_file(&database).await?;
    let sources = FileNoteStore::open(backup)?;
    for (note_id, source) in sources.list_sources()? {
        let source = std::str::from_utf8(&source)?;
        let projection = parse_note_projection(source)
            .map_err(|_| format!("backup note source is invalid: {note_id}"))?;
        if projection.note_id != note_id {
            return Err(format!("backup note ID does not match its file name: {note_id}").into());
        }
    }

    let layout = StorageLayout::open(data_directory)?;
    let result = restore_into_validated(backup, data_directory);
    if let Err(error) = result {
        tracing::error!(output = %data_directory.display(), error = %error, "restore preparation failed; incomplete output was retained");
        return Err(error);
    }
    debug_assert_eq!(layout.data_directory(), data_directory);
    tracing::info!(input = %backup.display(), output = %data_directory.display(), "restore preparation completed");
    Ok(())
}

fn restore_into_validated(
    backup: &Path,
    data_directory: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::copy(
        backup.join("marginalis.sqlite"),
        data_directory.join("marginalis.sqlite"),
    )?;
    let sources = FileNoteStore::open(backup)?;
    sources.copy_sources_to(data_directory)?;
    std::fs::write(
        data_directory.join("RESTORED"),
        "Marginalis restore format 1\n",
    )?;
    Ok(())
}

/// backupのformatと内容を、復元前に機械的に検証できる固定manifestへ記録する。
///
/// `SourceRevision`はSHA-256のhex表現なので、SQLiteと正本の同一性確認にも再利用する。
fn write_backup_manifest(
    backup: &Path,
    created_at: UnixMillis,
) -> Result<(), Box<dyn std::error::Error>> {
    let database = std::fs::read(backup.join("marginalis.sqlite"))?;
    let sources = FileNoteStore::open(backup)?;
    let mut manifest = format!(
        "marginalis-backup-format=1\ncreated-at-ms={}\ndatabase-sha256={}\n",
        created_at.get(),
        marginalis_domain::SourceRevision::from_source(&database).to_hex()
    );
    for (note_id, source) in sources.list_sources()? {
        manifest.push_str(&format!(
            "note-sha256={note_id} {}\n",
            marginalis_domain::SourceRevision::from_source(&source).to_hex()
        ));
    }
    std::fs::write(backup.join("MANIFEST"), manifest)?;
    std::fs::File::open(backup)?.sync_all()?;
    Ok(())
}

fn verify_backup_manifest(backup: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = std::fs::read_to_string(backup.join("MANIFEST"))?;
    let mut lines = manifest.lines();
    if lines.next() != Some("marginalis-backup-format=1") {
        return Err("backup manifest format is missing or unsupported".into());
    }
    let created_at = lines
        .next()
        .and_then(|line| line.strip_prefix("created-at-ms="))
        .ok_or("backup manifest creation time is missing")?;
    created_at
        .parse::<i64>()
        .map_err(|_| "backup manifest creation time is invalid")?;
    let expected_database = lines
        .next()
        .and_then(|line| line.strip_prefix("database-sha256="))
        .ok_or("backup manifest database hash is missing")?;
    let actual_database = marginalis_domain::SourceRevision::from_source(&std::fs::read(
        backup.join("marginalis.sqlite"),
    )?)
    .to_hex();
    if expected_database != actual_database {
        return Err("backup database does not match its manifest".into());
    }

    let mut expected_notes = BTreeMap::new();
    for line in lines {
        let entry = line
            .strip_prefix("note-sha256=")
            .ok_or("backup manifest contains an unknown entry")?;
        let (note_id, revision) = entry
            .split_once(' ')
            .ok_or("backup manifest note entry is invalid")?;
        if note_id.is_empty()
            || revision.len() != 64
            || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            || expected_notes
                .insert(note_id.to_owned(), revision.to_owned())
                .is_some()
        {
            return Err("backup manifest note entry is invalid".into());
        }
    }
    let sources = FileNoteStore::open(backup)?;
    let actual_notes = sources
        .list_sources()?
        .into_iter()
        .map(|(note_id, source)| {
            (
                note_id.to_string(),
                marginalis_domain::SourceRevision::from_source(&source).to_hex(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if expected_notes != actual_notes {
        return Err("backup canonical sources do not match their manifest".into());
    }
    Ok(())
}

/// 停止中のserviceに対して実行する、正本からのSQLite投影再構築コマンド。
async fn rebuild_projections() -> Result<(), Box<dyn std::error::Error>> {
    let configuration = StorageConfig::from_environment()?;
    let layout = StorageLayout::open(&configuration.data_dir)?;
    let database = SqliteDatabase::connect_with_initial_registration_policy(
        &configuration.database_url,
        configuration.initial_registration_policy,
    )
    .await?;
    let sources = FileNoteStore::open(layout.data_directory())?;
    let notes = ServerNoteUseCases::new(database, sources);
    notes.recover().await?;
    let count = notes.rebuild_projections().await?;
    tracing::info!(count, "note projections rebuilt from canonical sources");
    Ok(())
}

/// root監査を365日で保持する定期maintenance command。
async fn prune_audit() -> Result<(), Box<dyn std::error::Error>> {
    let configuration = StorageConfig::from_environment()?;
    StorageLayout::open(&configuration.data_dir)?;
    let database = SqliteDatabase::connect_with_initial_registration_policy(
        &configuration.database_url,
        configuration.initial_registration_policy,
    )
    .await?;
    let retention_ms = 365_i64 * 24 * 60 * 60 * 1_000;
    let cutoff = UnixMillis::new(SystemClock.now().get().saturating_sub(retention_ms));
    let purged = database.purge_root_audit_before(cutoff).await?;
    tracing::info!(purged, "expired root audit records purged");
    Ok(())
}

fn initialize_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,marginalis_auth_oidc=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (configuration, secrets) = ServerConfig::from_environment()?;
    let layout = StorageLayout::open(&configuration.storage.data_dir)?;
    let database = SqliteDatabase::connect_with_initial_registration_policy(
        &configuration.storage.database_url,
        configuration.storage.initial_registration_policy,
    )
    .await?;
    let sources = FileNoteStore::open(layout.data_directory())?;
    let notes = ServerNoteUseCases::new(database.clone(), sources);
    notes.recover().await?;
    let root_store = database.root_credential_store();
    if !root_store.is_initialized().await? {
        let password = secrets.initial_root_password.ok_or(
            "ROOT_PASSWORD or ROOT_PASSWORD_FILE is required for an uninitialized database",
        )?;
        RootInitializationService::new(&root_store, &SystemRandom, &SystemClock)
            .initialize_if_missing(password)
            .await?;
    }
    let oidc_configuration = OidcConfiguration::new(
        configuration.oidc.issuer_url.to_string(),
        configuration.oidc.client_id,
        secrets.oidc_client_secret,
        configuration.http.base_url.as_str(),
    )?;
    let oidc = match OidcAuthentication::discover(&oidc_configuration).await {
        Ok(oidc) => Some(oidc),
        Err(error) => {
            tracing::error!(error = %error, "OIDC discovery failed; starting with root login only");
            None
        }
    };
    let resource_uri = base_url_at(&configuration.http.base_url, "mcp");
    let metadata_uri = base_url_at(
        &configuration.http.base_url,
        ".well-known/oauth-protected-resource/mcp",
    );
    let authorization_endpoint_uri = base_url_at(&configuration.http.base_url, "oauth/authorize");
    let token_endpoint_uri = base_url_at(&configuration.http.base_url, "oauth/token");
    let listener = tokio::net::TcpListener::bind(configuration.http.listen_address).await?;
    tracing::info!(address = %configuration.http.listen_address, "Marginalis server listening");
    let cookie_path = cookie_path(&configuration.http.base_url);
    let authentication = std::sync::Arc::new(match oidc {
        Some(oidc) => ServerWebAuthenticationUseCases::with_oidc_and_cookie_path(
            database.clone(),
            oidc,
            cookie_path.clone(),
        ),
        None => ServerWebAuthenticationUseCases::with_cookie_path(database.clone(), cookie_path),
    });
    let state = ApiState::new(
        std::sync::Arc::new(notes.clone()),
        authentication.clone(),
        authentication.clone(),
        authentication,
        configuration.http.base_url.origin().ascii_serialization(),
    );
    let state = if configuration.mcp_enabled {
        let oauth = std::sync::Arc::new(ServerMcpOAuthService::new(
            database.clone(),
            configuration.mcp_client_metadata_allowed_hosts,
        ));
        state.with_mcp(McpEndpoint {
            tools: marginalis_mcp::McpTools::new(std::sync::Arc::new(notes)),
            authenticator: std::sync::Arc::new(ServerMcpAuthenticator::new(
                database,
                resource_uri.to_string(),
            )),
            oauth: oauth.clone(),
            oauth_administration: oauth,
            resource_uri: resource_uri.to_string(),
            metadata_uri: metadata_uri.to_string(),
            authorization_server_uri: configuration.http.base_url.to_string(),
            authorization_endpoint_uri: authorization_endpoint_uri.to_string(),
            token_endpoint_uri: token_endpoint_uri.to_string(),
            allowed_origin: configuration.http.base_url.origin().ascii_serialization(),
            rate_limiter: marginalis_web::McpRateLimiter::new(120),
        })
    } else {
        state
    };
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn base_url_at(base_url: &url::Url, suffix: &str) -> url::Url {
    let mut url = base_url.clone();
    url.set_path(&format!(
        "{}/{suffix}",
        base_url.path().trim_end_matches('/')
    ));
    url
}

fn cookie_path(base_url: &url::Url) -> String {
    let path = base_url.path().trim_end_matches('/');
    if path.is_empty() {
        "/".into()
    } else {
        path.into()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;

    #[tokio::test]
    async fn restore_prepares_a_verified_backup_without_changing_the_original() {
        let root = std::env::temp_dir().join(format!("marginalis-restore-{}", Uuid::now_v7()));
        let backup = root.join("backup");
        let output = root.join("restored");
        StorageLayout::open(&backup).expect("initialize backup layout");
        fs::create_dir_all(backup.join("notes")).expect("create backup");
        let database_source = root.join("source.sqlite");
        let database = SqliteDatabase::connect(&format!("sqlite:{}", database_source.display()))
            .await
            .expect("database");
        database
            .backup_to(
                backup
                    .join("marginalis.sqlite")
                    .to_str()
                    .expect("backup path"),
            )
            .await
            .expect("backup database");
        drop(database);
        let note_id = "01800000-0000-7000-8000-000000000095";
        fs::write(
            backup.join("notes").join(format!("{note_id}.adoc")),
            format!(
                "= Restored note\n:note-id: {note_id}\n:creator-id: 01800000-0000-7000-8000-000000000094\n:created-at: 2026-07-23T00:00:00.000Z\n:updated-at: 2026-07-23T00:00:00.000Z\n:tags: recovery\n\ncanonical body\n"
            ),
        )
        .expect("write note");
        write_backup_manifest(&backup, UnixMillis::new(0)).expect("write manifest");
        fs::write(backup.join("COMPLETE"), "Marginalis backup format 1\n").expect("marker");

        restore_into(&backup, &output).await.expect("restore");

        assert_eq!(
            fs::read(output.join("notes").join(format!("{note_id}.adoc")))
                .expect("restored source"),
            fs::read(backup.join("notes").join(format!("{note_id}.adoc"))).expect("backup source")
        );
        assert!(output.join("marginalis.sqlite").is_file());
        assert_eq!(
            fs::read_to_string(output.join("RESTORED")).expect("restore marker"),
            "Marginalis restore format 1\n"
        );
        StorageLayout::validate_existing(&output).expect("restored format marker");
        assert!(backup.join("marginalis.sqlite").is_file());
        fs::remove_dir_all(root).expect("remove test files");
    }

    #[test]
    fn backup_manifest_rejects_changed_canonical_source() {
        let root = std::env::temp_dir().join(format!("marginalis-manifest-{}", Uuid::now_v7()));
        StorageLayout::open(&root).expect("initialize layout");
        fs::write(root.join("marginalis.sqlite"), "not a database").expect("write database");
        let note_id = "01800000-0000-7000-8000-000000000096";
        fs::create_dir_all(root.join("notes")).expect("create notes");
        fs::write(
            root.join("notes").join(format!("{note_id}.adoc")),
            "= Original\n",
        )
        .expect("write source");
        write_backup_manifest(&root, UnixMillis::new(0)).expect("write manifest");
        fs::write(
            root.join("notes").join(format!("{note_id}.adoc")),
            "= Changed\n",
        )
        .expect("change source");

        assert!(verify_backup_manifest(&root).is_err());
        fs::remove_dir_all(root).expect("remove test files");
    }
}
