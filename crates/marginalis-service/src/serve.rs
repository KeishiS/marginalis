//! HTTP serviceのcomposition root。

use marginalis_asciidoc::verify_runtime_package_version;
use marginalis_auth_oidc::{OidcAuthentication, OidcConfiguration};
use marginalis_server::{
    ServerConfig, ServerMcpOAuthService, ServerNoteUseCases, ServerOidcAuthenticationUseCases,
    ServerWebSessionUseCases,
};
use marginalis_sqlite::SqliteDatabase;
use std::path::Path;

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
        Ok(oidc) => Some(oidc),
        Err(error) => {
            tracing::warn!(%error, "OIDC discovery is unavailable; login requests will fail closed");
            None
        }
    };
    let listener = tokio::net::TcpListener::bind(configuration.http.listen_address).await?;
    tracing::info!(address = %configuration.http.listen_address, "Marginalis server listening");
    let cookie_path = cookie_path(&configuration.http.base_url);
    let oidc = std::sync::Arc::new(ServerOidcAuthenticationUseCases::new(
        database.clone(),
        oidc_configuration,
        oidc_http_client,
        oidc,
    ));
    let sessions = std::sync::Arc::new(ServerWebSessionUseCases::new(
        database.clone(),
        marginalis_application::SessionLifetime {
            idle_timeout_ms: 24 * 60 * 60 * 1_000,
            absolute_timeout_ms: 7 * 24 * 60 * 60 * 1_000,
        },
    ));
    let notes = std::sync::Arc::new(ServerNoteUseCases::new(database.clone()));
    let state = marginalis_web::http::ApiState::new(
        notes.clone(),
        sessions,
        oidc,
        cookie_path,
        configuration.http.base_url.origin().ascii_serialization(),
    );
    let state = if configuration.mcp_enabled {
        let resource_uri = base_url_at(&configuration.http.base_url, "mcp");
        let metadata_uri = well_known_url(&resource_uri, "oauth-protected-resource");
        let authorization_server_metadata_uri =
            well_known_url(&configuration.http.base_url, "oauth-authorization-server");
        let authorization_endpoint_uri =
            base_url_at(&configuration.http.base_url, "oauth/authorize");
        let token_endpoint_uri = base_url_at(&configuration.http.base_url, "oauth/token");
        state.with_mcp(marginalis_web::http::McpEndpoint {
            oauth: std::sync::Arc::new(ServerMcpOAuthService::new(
                database,
                resource_uri.to_string(),
            )),
            notes,
            allowed_origins: configuration.mcp_allowed_origins,
            resource_uri: resource_uri.to_string(),
            metadata_uri: metadata_uri.to_string(),
            authorization_server_uri: configuration.http.base_url.to_string(),
            authorization_server_metadata_uri: authorization_server_metadata_uri.to_string(),
            authorization_endpoint_uri: authorization_endpoint_uri.to_string(),
            token_endpoint_uri: token_endpoint_uri.to_string(),
        })
    } else {
        state
    };
    axum::serve(
        listener,
        marginalis_web::http::router(state)
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

/// RFC 8414/9728に従い、subject URLのhostとpathの間へwell-known suffixを挿入する。
fn well_known_url(subject: &url::Url, suffix: &str) -> url::Url {
    let mut url = subject.clone();
    let subject_path = subject.path().trim_end_matches('/');
    url.set_path(
        if subject_path.is_empty() {
            format!("/.well-known/{suffix}")
        } else {
            format!("/.well-known/{suffix}{subject_path}")
        }
        .as_str(),
    );
    url
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_urls_insert_the_suffix_before_a_subject_path() {
        let root = url::Url::parse("https://example.test").expect("root URL");
        assert_eq!(
            well_known_url(&root, "oauth-authorization-server").as_str(),
            "https://example.test/.well-known/oauth-authorization-server"
        );

        let issuer = url::Url::parse("https://example.test/marginalis").expect("issuer URL");
        assert_eq!(
            well_known_url(&issuer, "oauth-authorization-server").as_str(),
            "https://example.test/.well-known/oauth-authorization-server/marginalis"
        );

        let resource =
            url::Url::parse("https://example.test/marginalis/mcp").expect("resource URL");
        assert_eq!(
            well_known_url(&resource, "oauth-protected-resource").as_str(),
            "https://example.test/.well-known/oauth-protected-resource/marginalis/mcp"
        );
    }
}
