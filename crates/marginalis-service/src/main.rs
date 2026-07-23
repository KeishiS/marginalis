//! Marginalisのcomposition root。設定読込、adapter組立、tracingおよびHTTP listenを担う。

use marginalis_application::{Clock, RootCredentialStore, RootInitializationService};
use marginalis_domain::UnixMillis;
use marginalis_files::FileNoteStore;
use marginalis_server::{
    ServerConfig, ServerMcpAuthenticator, ServerMcpOAuthService, ServerNoteUseCases,
    ServerWebAuthenticationUseCases, SystemClock, SystemRandom,
};
use marginalis_sqlite::SqliteDatabase;
use marginalis_web::{ApiState, McpEndpoint, OidcAuthentication, OidcConfiguration, router};
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    initialize_tracing();
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next();
    let result = match command.as_deref() {
        None | Some("serve") => run().await,
        Some("rebuild-projections") => rebuild_projections().await,
        Some("backup") => backup(arguments).await,
        Some(_) => Err(
            "usage: marginalis [serve|rebuild-projections|backup (--output <absolute-directory>|--directory <absolute-directory>)]"
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
    let (configuration, _) = ServerConfig::from_environment()?;
    let database = SqliteDatabase::connect_with_initial_registration_policy(
        &configuration.database_url,
        configuration.initial_registration_policy,
    )
    .await?;
    let sources = FileNoteStore::open(&configuration.data_dir)?;
    let database_path = output.join("marginalis.sqlite");
    let database_path = database_path
        .to_str()
        .ok_or("backup output directory must be valid UTF-8")?;
    database.backup_to(database_path).await?;
    let note_count = sources.copy_sources_to(output)?;
    std::fs::write(output.join("COMPLETE"), "Marginalis backup format 1\n")?;
    tracing::info!(output = %output.display(), note_count, "backup completed");
    Ok(())
}

/// 停止中のserviceに対して実行する、正本からのSQLite投影再構築コマンド。
async fn rebuild_projections() -> Result<(), Box<dyn std::error::Error>> {
    let (configuration, _) = ServerConfig::from_environment()?;
    std::fs::create_dir_all(&configuration.data_dir)?;
    let database = SqliteDatabase::connect_with_initial_registration_policy(
        &configuration.database_url,
        configuration.initial_registration_policy,
    )
    .await?;
    let sources = FileNoteStore::open(&configuration.data_dir)?;
    let notes = ServerNoteUseCases::new(database, sources);
    notes.recover().await?;
    let count = notes.rebuild_projections().await?;
    tracing::info!(count, "note projections rebuilt from canonical sources");
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
    std::fs::create_dir_all(&configuration.data_dir)?;
    let database = SqliteDatabase::connect_with_initial_registration_policy(
        &configuration.database_url,
        configuration.initial_registration_policy,
    )
    .await?;
    // root監査は365日保持する。古い行だけを起動時に掃除し、通常のHTTP APIからは公開しない。
    let retention_ms = 365_i64 * 24 * 60 * 60 * 1_000;
    let cutoff = UnixMillis::new(SystemClock.now().get().saturating_sub(retention_ms));
    let purged = database.purge_root_audit_before(cutoff).await?;
    if purged > 0 {
        tracing::info!(purged, "expired root audit records purged");
    }
    let sources = FileNoteStore::open(&configuration.data_dir)?;
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
        configuration.base_url.as_str(),
    )?;
    let oidc = match OidcAuthentication::discover(&oidc_configuration).await {
        Ok(oidc) => Some(oidc),
        Err(error) => {
            tracing::error!(error = %error, "OIDC discovery failed; starting with root login only");
            None
        }
    };
    let resource_uri = base_url_at(&configuration.base_url, "mcp");
    let metadata_uri = base_url_at(
        &configuration.base_url,
        ".well-known/oauth-protected-resource/mcp",
    );
    let authorization_endpoint_uri = base_url_at(&configuration.base_url, "oauth/authorize");
    let token_endpoint_uri = base_url_at(&configuration.base_url, "oauth/token");
    let listener = tokio::net::TcpListener::bind(configuration.listen_address).await?;
    tracing::info!(address = %configuration.listen_address, "Marginalis server listening");
    let state = ApiState::new(
        std::sync::Arc::new(notes.clone()),
        std::sync::Arc::new(match oidc {
            Some(oidc) => ServerWebAuthenticationUseCases::with_oidc(database.clone(), oidc),
            None => ServerWebAuthenticationUseCases::new(database.clone()),
        }),
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
            authorization_server_uri: configuration.base_url.to_string(),
            authorization_endpoint_uri: authorization_endpoint_uri.to_string(),
            token_endpoint_uri: token_endpoint_uri.to_string(),
            allowed_origin: configuration.base_url.origin().ascii_serialization(),
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
