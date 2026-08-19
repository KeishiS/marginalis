//! HTTP serviceのcomposition root。

use marginalis_application::{
    Clock, McpOAuthApplication, McpResourcePolicy, NoteApplication, NoteApplicationDependencies,
    OidcAuthenticationApplication, WebSessionApplication,
};
use marginalis_asciidoc::{AsciiDocNoteContent, verify_runtime_package_version};
use marginalis_auth_oidc::{OidcAuthentication, OidcConfiguration, OidcIdentityProvider};
use marginalis_sqlite::SqliteDatabase;
use mcp_authorization_server_cimd::HttpClientMetadataResolver;
use std::path::Path;

use crate::{
    config::ServerConfig,
    runtime::{SystemClock, SystemRandom},
};

/// Webセッションを最終利用から失効させるまでの時間（`REQ-AUTH-007`）。
const SESSION_IDLE_TIMEOUT_MS: i64 = 24 * 60 * 60 * 1_000;
/// Webセッションをログインから失効させるまでの時間（`REQ-AUTH-007`）。
const SESSION_ABSOLUTE_TIMEOUT_MS: i64 = 7 * 24 * 60 * 60 * 1_000;

pub(crate) async fn run() -> Result<(), Box<dyn std::error::Error>> {
    verify_runtime_package_version()?;
    let (configuration, secrets) = ServerConfig::from_environment()?;
    let database = SqliteDatabase::connect(&configuration.storage.database_url).await?;
    let oidc_configuration = OidcConfiguration::new(
        configuration.oidc.issuer_url.to_string(),
        configuration.oidc.client_id.clone(),
        secrets.oidc_client_secret,
        configuration.http.base_url.as_str(),
    )?
    .with_scopes(configuration.oidc.scopes.clone())
    .with_group_claim(configuration.oidc.group_claim.clone())?
    .with_allowed_algorithms(
        configuration
            .oidc
            .allowed_algorithms
            .iter()
            .map(|algorithm| match algorithm {
                crate::config::OidcSigningAlgorithm::Es256 => {
                    marginalis_auth_oidc::OidcSigningAlgorithm::EcdsaP256Sha256
                }
                crate::config::OidcSigningAlgorithm::Rs256 => {
                    marginalis_auth_oidc::OidcSigningAlgorithm::RsaSsaPkcs1V15Sha256
                }
            })
            .collect(),
    )?
    .with_token_endpoint_auth(match configuration.oidc.token_endpoint_auth {
        crate::config::OidcTokenEndpointAuthMethod::ClientSecretPost => {
            marginalis_auth_oidc::OidcTokenEndpointAuth::ClientSecretPost
        }
        crate::config::OidcTokenEndpointAuthMethod::ClientSecretBasic => {
            marginalis_auth_oidc::OidcTokenEndpointAuth::ClientSecretBasic
        }
    });
    let oidc_http_client = oidc_provider_http_client(
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
                reason = "unavailable",
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
        configuration.oidc.allowed_claim_values.clone(),
    ));
    // すべてのrepository portを同じSQLite adapterが担うため、Arcを1つ作って共有する。
    let storage = std::sync::Arc::new(database.clone());
    let sessions = std::sync::Arc::new(WebSessionApplication::new(
        storage.clone(),
        std::sync::Arc::new(SystemClock),
        std::sync::Arc::new(SystemRandom),
        marginalis_application::SessionLifetime {
            idle_timeout_ms: SESSION_IDLE_TIMEOUT_MS,
            absolute_timeout_ms: SESSION_ABSOLUTE_TIMEOUT_MS,
        },
    ));
    let notes = std::sync::Arc::new(NoteApplication::new(
        NoteApplicationDependencies::with_storage(
            &storage,
            std::sync::Arc::new(AsciiDocNoteContent),
            std::sync::Arc::new(marginalis_web::http::HttpNoteLinkResolver),
            std::sync::Arc::new(SystemClock),
            std::sync::Arc::new(SystemRandom),
        ),
    ));
    let bibliography = std::sync::Arc::new(marginalis_application::BibliographyApplication::new(
        storage.clone(),
        std::sync::Arc::new(SystemClock),
        std::sync::Arc::new(SystemRandom),
    ));
    let bibliography_import =
        std::sync::Arc::new(marginalis_application::BibliographyImportApplication::new(
            storage.clone(),
            std::sync::Arc::new(SystemClock),
            std::sync::Arc::new(SystemRandom),
        ));
    let math_macros = std::sync::Arc::new(marginalis_application::MathMacroApplication::new(
        storage.clone(),
    ));
    // 送信adapterは配送workerと購読の所有確認で同じ検査条件を使うため共有する。
    let webhook_allowed_hosts =
        crate::environment::comma_separated(crate::environment::WEBHOOK_ALLOWED_HOSTS);
    let webhook_sender = std::sync::Arc::new(marginalis_webhook_http::WebhookHttpSender::new(
        webhook_allowed_hosts.clone(),
    ));
    let webhooks =
        std::sync::Arc::new(marginalis_application::WebhookSubscriptionApplication::new(
            storage.clone(),
            webhook_sender.clone(),
            std::sync::Arc::new(SystemClock),
            std::sync::Arc::new(SystemRandom),
            webhook_allowed_hosts,
        ));
    let state = marginalis_web::http::ApiState::new(
        notes.clone(),
        math_macros,
        sessions,
        oidc,
        cookie_path,
        configuration.http.base_url.origin().ascii_serialization(),
    )
    .with_bibliography(bibliography)
    .with_bibliography_import(bibliography_import)
    .with_webhooks(webhooks);
    let state = if configuration.mcp_enabled {
        let resource_uri =
            marginalis_web::http::McpEndpoint::resource_uri_for(&configuration.http.base_url);
        let resource_policy = McpResourcePolicy::new(
            resource_uri,
            "Marginalis MCP".into(),
            vec![
                "notes:read".into(),
                "notes:write".into(),
                "notes:delete".into(),
                "notes:sync".into(),
                "bibliography:read".into(),
                "bibliography:write".into(),
                "bibliography:delete".into(),
            ],
            vec!["notes:read".into()],
        )?;
        let endpoint = marginalis_web::http::McpEndpoint::new(
            std::sync::Arc::new(
                McpOAuthApplication::new(
                    storage.clone(),
                    storage.clone(),
                    std::sync::Arc::new(SystemClock),
                    std::sync::Arc::new(SystemRandom),
                    resource_policy,
                )
                .with_client_metadata_resolver(std::sync::Arc::new(
                    HttpClientMetadataResolver::new(std::time::Duration::from_secs(5)),
                )),
            ),
            &configuration.http.base_url,
            configuration.mcp_allowed_origins,
        )?;
        state.with_mcp(endpoint)
    } else {
        state
    };
    // 停止シグナルをHTTP listenerとWebhook配送workerで共有する。
    let (shutdown_sender, shutdown_receiver) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = shutdown_sender.send(true);
    });
    let worker =
        spawn_webhook_delivery_worker(storage.clone(), webhook_sender, shutdown_receiver.clone());
    let listener = tokio::net::TcpListener::bind(configuration.http.listen_address).await?;
    tracing::info!(event = "service.listening", address = %configuration.http.listen_address, "Marginalis server listening");
    let mut http_shutdown = shutdown_receiver;
    axum::serve(
        listener,
        marginalis_web::http::router(state)
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = http_shutdown.changed().await;
    })
    .await?;
    let _ = worker.await;
    tracing::info!(
        event = "service.shutdown.completed",
        "HTTP listener stopped after draining requests"
    );
    Ok(())
}

/// Webhook配送workerを常駐taskとして起動する。
///
/// 1秒間隔で期限の来た配送を処理し、停止シグナルで実行中のtickを終えてから
/// 抜ける。結果のログはここで記録し、application層はログに依存しない。
fn spawn_webhook_delivery_worker(
    storage: std::sync::Arc<SqliteDatabase>,
    sender: std::sync::Arc<marginalis_webhook_http::WebhookHttpSender>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!(
            event = "webhook.worker.started",
            "webhook delivery worker started"
        );
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = interval.tick() => {}
            }
            if *shutdown.borrow() {
                break;
            }
            let now = SystemClock.now();
            match marginalis_application::webhook_delivery_tick(
                storage.as_ref(),
                sender.as_ref(),
                now,
            )
            .await
            {
                Ok(outcome) => {
                    for webhook_id in &outcome.delivered {
                        tracing::info!(
                            event = "webhook.delivery.succeeded",
                            webhook_id = %webhook_id,
                            "webhook event delivered"
                        );
                    }
                    for (webhook_id, failure) in &outcome.failed {
                        tracing::warn!(
                            event = "webhook.delivery.failed",
                            webhook_id = %webhook_id,
                            reason = failure.as_str(),
                            "webhook delivery failed and will retry until the limit"
                        );
                    }
                    for webhook_id in &outcome.disabled {
                        tracing::warn!(
                            event = "webhook.subscription.disabled",
                            webhook_id = %webhook_id,
                            reason = "delivery_exhausted",
                            "webhook subscription disabled after exhausting retries"
                        );
                    }
                }
                Err(error) => {
                    tracing::error!(
                        event = "webhook.worker.tick_failed",
                        error = %error,
                        "webhook delivery tick failed"
                    );
                }
            }
        }
        tracing::info!(
            event = "webhook.worker.stopped",
            "webhook delivery worker drained and stopped"
        );
    })
}

async fn shutdown_signal() {
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(
                event = "service.signal_handler.failed",
                reason = "ctrl-c",
                error = %error,
                "failed to install Ctrl-C handler"
            );
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
                tracing::error!(
                    event = "service.signal_handler.failed",
                    reason = "sigterm",
                    error = %error,
                    "failed to install SIGTERM handler"
                );
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
    tracing::info!(
        event = "service.shutdown.started",
        "shutdown signal received; draining HTTP requests"
    );
}

fn oidc_provider_http_client(
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
