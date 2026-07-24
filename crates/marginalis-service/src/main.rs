//! Marginalisのcomposition root。設定読込、adapter組立、tracingおよびHTTP listenを担う。

use marginalis_application::{
    AuthenticationUseCaseError, Clock, V3GroupMembership, V3MembershipResolver,
};
use marginalis_asciidoc::verify_runtime_package_version;
use marginalis_auth_oidc::{OidcAuthentication, OidcConfiguration};
use marginalis_domain::UnixMillis;
use marginalis_server::{
    ServerConfig, ServerV3McpOAuthService, ServerV3NoteUseCases,
    ServerV3OidcAuthenticationUseCases, ServerV3WebSessionUseCases, StorageConfig, SystemClock,
};
use marginalis_sqlite::V3SqliteDatabase;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: marginalis [--version|serve|purge-deleted|export-archive --output <absolute-file>|import-archive --input <absolute-file>|backup (--output <absolute-directory>|--directory <absolute-directory>)]";
const V3_SOFT_DELETE_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

#[tokio::main]
async fn main() {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next();
    if matches!(command.as_deref(), Some("--version" | "-V")) && arguments.next().is_none() {
        println!("marginalis {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    initialize_tracing();
    let result = match command.as_deref() {
        None | Some("serve") => run().await,
        Some("purge-deleted") => purge_deleted().await,
        Some("export-archive") => export_archive(arguments).await,
        Some("import-archive") => import_archive(arguments).await,
        Some("backup") => backup(arguments).await,
        Some(_) => Err(USAGE.into()),
    };
    if let Err(error) = result {
        tracing::error!(error = %error, "Marginalis server terminated");
        std::process::exit(1);
    }
}

/// v0.3の30日間ソフトデリート保持期限を過ぎた正本を物理削除する。
///
/// 実行中のHTTP serviceと並行してもSQLite transactionとして完結する。NixOS timerは日次で起動する。
async fn purge_deleted() -> Result<(), Box<dyn std::error::Error>> {
    let configuration = StorageConfig::from_environment()?;
    let database = V3SqliteDatabase::connect(&configuration.database_url).await?;
    let cutoff = UnixMillis::new(SystemClock.now().get() - V3_SOFT_DELETE_RETENTION_MS);
    let count = database.purge_deleted_before(cutoff).await?;
    tracing::info!(
        count,
        cutoff_ms = cutoff.get(),
        "purged expired soft-deleted v3 notes"
    );
    Ok(())
}

/// SQLite正本を、ACL・削除状態を含む検証可能なv0.3 archiveとして出力する。
async fn export_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = required_absolute_file_argument(&mut arguments, "--output")?;
    if output.exists() {
        return Err(format!("archive output already exists: {}", output.display()).into());
    }
    let configuration = StorageConfig::from_environment()?;
    let archive = V3SqliteDatabase::connect(&configuration.database_url)
        .await?
        .export_archive()
        .await?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    serde_json::to_writer_pretty(&file, &archive)?;
    file.sync_all()?;
    tracing::info!(output = %output.display(), note_count = archive.notes.len(), "exported v3 archive");
    Ok(())
}

/// archiveを全件検証してから、空のv0.3 SQLite databaseへ一transactionで取り込む。
async fn import_archive(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_absolute_file_argument(&mut arguments, "--input")?;
    let file = std::fs::File::open(&input)?;
    let archive = serde_json::from_reader(file)?;
    let configuration = StorageConfig::from_environment()?;
    V3SqliteDatabase::connect(&configuration.database_url)
        .await?
        .import_archive(&archive)
        .await?;
    tracing::info!(input = %input.display(), "imported v3 archive");
    Ok(())
}

fn required_absolute_file_argument(
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let received_option = arguments.next();
    let value = arguments.next();
    if received_option.as_deref() != Some(option) || value.is_none() || arguments.next().is_some() {
        return Err(format!("usage requires {option} <absolute-file>").into());
    }
    let path = PathBuf::from(value.expect("value was checked"));
    if !path.is_absolute() {
        return Err(format!("{option} must be an absolute file path").into());
    }
    Ok(path)
}

/// 停止中のserviceに対してSQLite正本を可搬archiveとして取得するbackup command。
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
    let archive = V3SqliteDatabase::connect(&configuration.database_url)
        .await?
        .export_archive()
        .await?;
    let archive_path = output.join("marginalis-v3-archive.json");
    let archive_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&archive_path)?;
    serde_json::to_writer_pretty(&archive_file, &archive)?;
    archive_file.sync_all()?;
    std::fs::write(
        output.join("COMPLETE"),
        format!(
            "Marginalis backup {}\n",
            marginalis_domain::CANONICAL_ARCHIVE_FORMAT
        ),
    )?;
    let note_count = archive.notes.len();
    tracing::info!(output = %output.display(), note_count, "backup completed");
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
    run_v3().await
}

/// v0.3.0のcomposition root。旧ファイル正本・root管理・`/api/v1`を組み立てない。
async fn run_v3() -> Result<(), Box<dyn std::error::Error>> {
    verify_runtime_package_version()?;
    let (configuration, secrets) = ServerConfig::from_environment()?;
    let database = V3SqliteDatabase::connect(&configuration.storage.database_url).await?;
    let oidc_configuration = OidcConfiguration::new(
        configuration.oidc.issuer_url.to_string(),
        configuration.oidc.client_id,
        secrets.oidc_client_secret,
        configuration.http.base_url.as_str(),
    )?;
    let oidc = match OidcAuthentication::discover(&oidc_configuration).await {
        Ok(oidc) => Some(oidc),
        Err(error) => {
            tracing::warn!(%error, "OIDC discovery is unavailable; login requests will fail closed");
            None
        }
    };
    let listener = tokio::net::TcpListener::bind(configuration.http.listen_address).await?;
    tracing::info!(address = %configuration.http.listen_address, "Marginalis server listening");
    let cookie_path = cookie_path(&configuration.http.base_url);
    let oidc = std::sync::Arc::new(ServerV3OidcAuthenticationUseCases::new(
        database.clone(),
        oidc_configuration,
        oidc,
    ));
    let membership = std::sync::Arc::new(KanidmMembershipResolver::new(
        configuration.oidc.membership_api_url.clone(),
        secrets.kanidm_membership_token,
    )?);
    let sessions = std::sync::Arc::new(ServerV3WebSessionUseCases::new(
        database.clone(),
        membership.clone(),
        marginalis_application::SessionLifetime {
            idle_timeout_ms: 24 * 60 * 60 * 1_000,
            absolute_timeout_ms: 7 * 24 * 60 * 60 * 1_000,
        },
    ));
    let notes = std::sync::Arc::new(ServerV3NoteUseCases::new(database.clone()));
    let state = marginalis_web::v3::V3ApiState::new(
        notes.clone(),
        sessions,
        oidc,
        cookie_path,
        configuration.http.base_url.origin().ascii_serialization(),
    );
    let state = if configuration.mcp_enabled {
        let resource_uri = base_url_at(&configuration.http.base_url, "mcp");
        let metadata_uri = base_url_at(
            &configuration.http.base_url,
            ".well-known/oauth-protected-resource/mcp",
        );
        let authorization_endpoint_uri =
            base_url_at(&configuration.http.base_url, "oauth/authorize");
        let token_endpoint_uri = base_url_at(&configuration.http.base_url, "oauth/token");
        state.with_mcp(marginalis_web::v3::V3McpEndpoint {
            oauth: std::sync::Arc::new(ServerV3McpOAuthService::with_membership(
                database, membership,
            )),
            notes,
            resource_uri: resource_uri.to_string(),
            metadata_uri: metadata_uri.to_string(),
            authorization_server_uri: configuration.http.base_url.to_string(),
            authorization_endpoint_uri: authorization_endpoint_uri.to_string(),
            token_endpoint_uri: token_endpoint_uri.to_string(),
        })
    } else {
        state
    };
    axum::serve(
        listener,
        marginalis_web::v3::router(state)
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn base_url_at(base_url: &url::Url, suffix: &str) -> url::Url {
    let mut url = base_url.clone();
    let prefix = base_url.path().trim_matches('/');
    url.set_path(
        if prefix.is_empty() {
            format!("/{suffix}")
        } else {
            format!("/{prefix}/{suffix}")
        }
        .as_str(),
    );
    url
}

/// Kanidm REST APIをread-only service accountで照会する。ネットワーク・応答形式の異常は
/// `Unavailable` とし、5分のfreshnessを越えたsessionをfail closedにする。
struct KanidmMembershipResolver {
    api_url: url::Url,
    token: String,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct KanidmEntry {
    #[serde(default)]
    attrs: BTreeMap<String, Vec<String>>,
}

impl KanidmMembershipResolver {
    fn new(api_url: url::Url, token: String) -> Result<Self, reqwest::Error> {
        Ok(Self {
            api_url,
            token,
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(5))
                .build()?,
        })
    }

    fn person_url(&self, subject: &str) -> Result<url::Url, AuthenticationUseCaseError> {
        let mut url = self.api_url.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        segments.pop_if_empty();
        segments.extend(["v1", "person", subject]);
        drop(segments);
        Ok(url)
    }
}

#[async_trait::async_trait]
impl V3MembershipResolver for KanidmMembershipResolver {
    async fn resolve(
        &self,
        _issuer: &str,
        subject: &str,
    ) -> Result<V3GroupMembership, AuthenticationUseCaseError> {
        let response = self
            .client
            .get(self.person_url(subject)?)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        if matches!(
            response.status(),
            reqwest::StatusCode::NOT_FOUND
                | reqwest::StatusCode::UNAUTHORIZED
                | reqwest::StatusCode::FORBIDDEN
        ) {
            return Err(AuthenticationUseCaseError::Rejected);
        }
        if !response.status().is_success() {
            return Err(AuthenticationUseCaseError::Unavailable);
        }
        let entry = response
            .json::<KanidmEntry>()
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        let groups = entry
            .attrs
            .get("memberof")
            .or_else(|| entry.attrs.get("memberOf"));
        let contains = |expected: &str| {
            groups.is_some_and(|groups| {
                groups.iter().any(|group| {
                    group == expected
                        || group.strip_suffix("@localhost") == Some(expected)
                        || group.split('@').next() == Some(expected)
                })
            })
        };
        Ok(V3GroupMembership {
            is_user: contains("server-users"),
            is_administrator: contains("server-admins"),
        })
    }
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
    use axum::{Router, extract::Path, http::HeaderMap, routing::get};

    use super::*;

    #[test]
    fn archive_arguments_require_exactly_one_absolute_file() {
        assert_eq!(
            required_absolute_file_argument(
                &mut [
                    "--output".to_owned(),
                    "/var/backups/archive.json".to_owned()
                ]
                .into_iter(),
                "--output",
            )
            .expect("absolute output"),
            PathBuf::from("/var/backups/archive.json")
        );
        assert!(
            required_absolute_file_argument(
                &mut ["--output".to_owned(), "relative.json".to_owned()].into_iter(),
                "--output",
            )
            .is_err()
        );
        assert!(
            required_absolute_file_argument(&mut ["--input".to_owned()].into_iter(), "--output")
                .is_err()
        );
    }

    #[tokio::test]
    async fn kanidm_membership_resolver_reads_the_service_account_view() {
        async fn person(
            Path(subject): Path<String>,
            headers: HeaderMap,
        ) -> axum::Json<serde_json::Value> {
            assert_eq!(subject, "subject-1");
            assert_eq!(
                headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok()),
                Some("Bearer service-token")
            );
            axum::Json(
                serde_json::json!({"attrs":{"memberof":["server-users@example.test", "server-admins@example.test"]}}),
            )
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/v1/person/{subject}", get(person)),
            )
            .await
            .expect("server");
        });
        let resolver = KanidmMembershipResolver::new(
            format!("http://{address}").parse().expect("URL"),
            "service-token".into(),
        )
        .expect("resolver");
        assert_eq!(
            resolver
                .resolve("https://id.example.test", "subject-1")
                .await
                .expect("membership"),
            V3GroupMembership {
                is_user: true,
                is_administrator: true,
            }
        );
    }
}
