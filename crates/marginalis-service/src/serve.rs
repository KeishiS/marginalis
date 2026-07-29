//! HTTP serviceのcomposition root。

use marginalis_application::{
    NoteApplication, OidcAuthenticationApplication, WebSessionApplication,
};
use marginalis_asciidoc::{AsciiDocNoteContent, verify_runtime_package_version};
use marginalis_auth_oauth::{McpAccessTokenAuthenticatorAdapter, McpAuthorizationConfiguration};
use marginalis_auth_oidc::{OidcAuthentication, OidcConfiguration, OidcIdentityProvider};
use marginalis_sqlite::SqliteDatabase;
use std::path::Path;

use crate::{
    config::ServerConfig,
    runtime::{SystemClock, SystemRandom},
};
pub(crate) async fn run() -> Result<(), Box<dyn std::error::Error>> {
    verify_runtime_package_version()?;
    let (configuration, secrets) = ServerConfig::from_environment()?;
    let database = SqliteDatabase::connect(&configuration.storage.database_url).await?;
    let oidc_configuration = OidcConfiguration::new(
        configuration.oidc.issuer_url.to_string(),
        configuration.oidc.client_id,
        secrets.oidc_client_secret,
        configuration.http.base_url.as_str(),
    )?;
    let oidc_http_client = kanidm_http_client(
        configuration.oidc.ca_certificate_file.as_deref(),
        std::time::Duration::from_secs(10),
    )?;
    let oidc = match OidcAuthentication::discover_with_http_client(
        &oidc_configuration,
        oidc_http_client.clone(),
    )
    .await
    {
        Ok(oidc) => {
            tracing::info!(
                event = "oidc.discovery.completed",
                "OIDC discovery succeeded"
            );
            Some(oidc)
        }
        Err(_error) => {
            tracing::warn!(
                event = "oidc.discovery.failed",
                error_kind = "unavailable",
                "OIDC discovery is unavailable; login requests will fail closed"
            );
            None
        }
    };
    let cookie_path = cookie_path(&configuration.http.base_url);
    let oidc_provider = OidcIdentityProvider::new(
        database.oidc_login_attempt_store(),
        SystemClock,
        SystemRandom,
        oidc_configuration,
        oidc_http_client,
        oidc,
    );
    let oidc = std::sync::Arc::new(OidcAuthenticationApplication::new(
        std::sync::Arc::new(oidc_provider),
        "server-users",
    ));
    let sessions = std::sync::Arc::new(WebSessionApplication::new(
        std::sync::Arc::new(database.clone()),
        std::sync::Arc::new(SystemClock),
        std::sync::Arc::new(SystemRandom),
        marginalis_application::SessionLifetime {
            idle_timeout_ms: 24 * 60 * 60 * 1_000,
            absolute_timeout_ms: 7 * 24 * 60 * 60 * 1_000,
        },
    ));
    let notes = std::sync::Arc::new(NoteApplication::new(
        std::sync::Arc::new(database.clone()),
        std::sync::Arc::new(database.clone()),
        std::sync::Arc::new(database.clone()),
        std::sync::Arc::new(AsciiDocNoteContent),
        std::sync::Arc::new(marginalis_web::http::HttpNoteLinkResolver),
        std::sync::Arc::new(SystemClock),
        std::sync::Arc::new(SystemRandom),
    ));
    let state = marginalis_web::http::ApiState::new(
        notes.clone(),
        sessions,
        oidc,
        cookie_path,
        configuration.http.base_url.origin().ascii_serialization(),
    );
    let state = if let Some(mcp) = configuration.mcp {
        let resource_uri =
            marginalis_web::http::McpEndpoint::resource_uri_for(&configuration.http.base_url);
        let authorization = mcp.authorization;
        let authenticator =
            match McpAccessTokenAuthenticatorAdapter::discover(McpAuthorizationConfiguration {
                issuer: authorization.issuer.clone(),
                audience: resource_uri,
                upstream_issuer: configuration.oidc.issuer_url.to_string(),
                upstream_issuer_claim: authorization.upstream_issuer_claim,
                upstream_subject_claim: authorization.upstream_subject_claim,
                groups_claim: authorization.groups_claim,
                required_user_group: "server-users".into(),
            })
            .await
            {
                Ok(authenticator) => std::sync::Arc::new(authenticator),
                Err(error) => {
                    let reason = match error {
                    marginalis_application::McpAccessTokenAuthenticationError::Configuration => {
                        "configuration"
                    }
                    marginalis_application::McpAccessTokenAuthenticationError::Discovery => {
                        "discovery"
                    }
                    marginalis_application::McpAccessTokenAuthenticationError::Rejected(_) => {
                        "rejected"
                    }
                    marginalis_application::McpAccessTokenAuthenticationError::Unavailable => {
                        "upstream-unavailable"
                    }
                };
                    tracing::error!(
                        event = "mcp.authorization.discovery.failed",
                        reason,
                        "MCP Authorization Server discovery failed"
                    );
                    return Err(error.into());
                }
            };
        tracing::info!(
            event = "mcp.authorization.discovery.completed",
            "MCP Authorization Server discovery succeeded"
        );
        let endpoint = marginalis_web::http::McpEndpoint::new(
            &configuration.http.base_url,
            mcp.allowed_origins,
            authorization.issuer,
            authenticator,
        )?;
        state.with_mcp(endpoint)
    } else {
        state
    };
    let listener = tokio::net::TcpListener::bind(configuration.http.listen_address).await?;
    tracing::info!(event = "service.listening", address = %configuration.http.listen_address, "Marginalis server listening");
    axum::serve(
        listener,
        marginalis_web::http::router(state)
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "failed to install Ctrl-C handler");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }
    tracing::info!("shutdown signal received; draining HTTP requests");
}

fn kanidm_http_client(
    ca_certificate_file: Option<&Path>,
    timeout: std::time::Duration,
) -> Result<reqwest::Client, Box<dyn std::error::Error>> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout);
    if let Some(path) = ca_certificate_file {
        builder =
            builder.add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(path)?)?);
    }
    Ok(builder.build()?)
}

fn cookie_path(base_url: &url::Url) -> String {
    let path = base_url.path().trim_end_matches('/');
    if path.is_empty() {
        "/".into()
    } else {
        path.into()
    }
}
