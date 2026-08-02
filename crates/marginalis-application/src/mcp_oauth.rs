//! MCP用OAuth Authorization Serverの業務処理と永続化port。

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_domain::{Actor, McpAuthenticatedActor, McpOAuthClient, UnixMillis};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use url::Url;

use crate::{
    Clock, McpAuthorizationClient, McpAuthorizationCodeExchange, McpAuthorizationRequest,
    McpClientRegistrationMethod, McpOAuthUseCaseError, McpOAuthUseCases, McpRefreshTokenRotation,
    McpRefreshTokenRotationOutcome, McpRegisteredOAuthClient, McpTokenPair,
    McpValidatedAuthorizationRequest, Random,
};

type McpIssuedTokenPair = McpTokenPair;
type McpOAuthError = McpOAuthUseCaseError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct McpOAuthRepositoryError;

/// OAuth client、code、token familyを原子的に保存する外向きport。
#[async_trait]
pub trait McpOAuthRepository: Send + Sync {
    async fn register_client_bounded(
        &self,
        client: &McpOAuthClient,
        now: UnixMillis,
        maximum_clients: i64,
    ) -> Result<bool, McpOAuthRepositoryError>;
    async fn client(
        &self,
        client_id: &str,
    ) -> Result<Option<McpRegisteredOAuthClient>, McpOAuthRepositoryError>;
    /// 認可codeを保存する。`client`は同じtransactionで登録し、外部keyを満たす。
    ///
    /// Client ID Metadata Documentで解決したclientは事前登録がないため、利用者が同意した
    /// この時点で初めて保存する。未認証の要求では保存しない。
    async fn issue_authorization_code(
        &self,
        code: &str,
        client: &McpRegisteredOAuthClient,
        grant: &marginalis_domain::McpAuthorizationGrant,
        code_challenge: &str,
        expires_at: UnixMillis,
        now: UnixMillis,
    ) -> Result<(), McpOAuthRepositoryError>;
    async fn exchange_authorization_code(
        &self,
        exchange: McpAuthorizationCodeExchange,
        now: UnixMillis,
    ) -> Result<Option<marginalis_domain::McpAuthorizationGrant>, McpOAuthRepositoryError>;
    async fn rotate_refresh_token(
        &self,
        rotation: McpRefreshTokenRotation,
        now: UnixMillis,
    ) -> Result<McpRefreshTokenRotationOutcome, McpOAuthRepositoryError>;
    async fn authenticate_access_token(
        &self,
        token: &str,
        resource_uri: &str,
        now: UnixMillis,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthRepositoryError>;
    async fn revoke_client_tokens(
        &self,
        issuer: &str,
        subject: &str,
        client_id: &str,
        now: UnixMillis,
    ) -> Result<(), McpOAuthRepositoryError>;
    async fn revoke_token(
        &self,
        token: &str,
        client_id: &str,
        now: UnixMillis,
    ) -> Result<(), McpOAuthRepositoryError>;
}

/// HTTPSのClient ID Metadata Documentを取得する外向きport。
#[async_trait]
pub trait McpClientMetadataResolver: Send + Sync {
    async fn resolve(
        &self,
        client_id: &str,
    ) -> Result<Option<McpOAuthClient>, McpOAuthRepositoryError>;
}

/// MCP OAuthのapplication service。
pub struct McpOAuthApplication {
    repository: Arc<dyn McpOAuthRepository>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
    resource_uri: String,
    client_metadata_resolver: Option<Arc<dyn McpClientMetadataResolver>>,
}

impl McpOAuthApplication {
    pub const ACCESS_TOKEN_SECONDS: u64 = 60 * 60;
    pub const REFRESH_TOKEN_SECONDS: u64 = 30 * 24 * 60 * 60;
    const AUTHORIZATION_CODE_SECONDS: i64 = 5 * 60;
    const MAX_DYNAMIC_CLIENTS: i64 = 1_000;

    pub fn new(
        repository: Arc<dyn McpOAuthRepository>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
        resource_uri: String,
    ) -> Self {
        Self {
            repository,
            clock,
            random,
            resource_uri,
            client_metadata_resolver: None,
        }
    }

    pub fn with_client_metadata_resolver(
        mut self,
        resolver: Arc<dyn McpClientMetadataResolver>,
    ) -> Self {
        self.client_metadata_resolver = Some(resolver);
        self
    }

    pub async fn register_client(
        &self,
        client: marginalis_domain::McpOAuthClient,
    ) -> Result<(), McpOAuthError> {
        validate_client_metadata(&client)?;
        let now = self.clock.now();
        let registered = self
            .repository
            .register_client_bounded(&client, now, Self::MAX_DYNAMIC_CLIENTS)
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        if !registered {
            return Err(McpOAuthError::Capacity);
        }
        Ok(())
    }

    pub async fn authorize(
        &self,
        actor: Actor,
        request: McpValidatedAuthorizationRequest,
    ) -> Result<String, McpOAuthError> {
        let code = self.random.opaque_token();
        let registered_client = McpRegisteredOAuthClient {
            client: request.client,
            registration_method: request.registration_method,
        };
        let grant = marginalis_domain::McpAuthorizationGrant {
            actor,
            client_id: registered_client.client.client_id.clone(),
            redirect_uri: request.redirect_uri,
            resource_uri: request.resource_uri,
            scopes: request.scopes,
        };
        let now = self.clock.now();
        self.repository
            .issue_authorization_code(
                &code,
                &registered_client,
                &grant,
                &request.code_challenge,
                UnixMillis::new(now.get() + Self::AUTHORIZATION_CODE_SECONDS * 1_000),
                now,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        Ok(code)
    }

    pub async fn validate_authorization_request(
        &self,
        request: &McpAuthorizationRequest,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthError> {
        let (resolved, registration_method) = self
            .resolve_client(&request.client_id, request.redirect_uri.as_deref())
            .await?;
        let client = resolved.client;
        let redirect_uri = resolved.redirect_uri;
        if !resource_uri_matches(&self.resource_uri, &request.resource_uri) {
            return Err(McpOAuthError::InvalidTarget);
        }
        let scopes = if request.scopes.is_empty() {
            vec!["notes:read".into()]
        } else if valid_mcp_scopes(&request.scopes) {
            canonical_scopes(&request.scopes)
        } else {
            return Err(McpOAuthError::InvalidScope);
        };
        if !valid_pkce_challenge(&request.code_challenge) {
            return Err(McpOAuthError::InvalidRequest);
        }
        Ok(McpValidatedAuthorizationRequest {
            client,
            registration_method,
            redirect_uri,
            resource_uri: self.resource_uri.clone(),
            scopes,
            code_challenge: request.code_challenge.clone(),
        })
    }

    pub async fn resolve_authorization_client(
        &self,
        client_id: &str,
        redirect_uri: Option<&str>,
    ) -> Result<McpAuthorizationClient, McpOAuthError> {
        self.resolve_client(client_id, redirect_uri)
            .await
            .map(|(client, _)| client)
    }

    async fn resolve_client(
        &self,
        client_id: &str,
        redirect_uri: Option<&str>,
    ) -> Result<(McpAuthorizationClient, McpClientRegistrationMethod), McpOAuthError> {
        let stored = self
            .repository
            .client(client_id)
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        let (client, registration_method) = match stored {
            Some(McpRegisteredOAuthClient {
                client,
                registration_method: McpClientRegistrationMethod::Dynamic,
            }) => (client, McpClientRegistrationMethod::Dynamic),
            Some(McpRegisteredOAuthClient {
                registration_method: McpClientRegistrationMethod::MetadataDocument,
                ..
            })
            | None => {
                if !valid_client_metadata_document_url(client_id) {
                    return Err(McpOAuthError::InvalidClient);
                }
                let Some(resolver) = &self.client_metadata_resolver else {
                    return Err(McpOAuthError::InvalidClient);
                };
                let Some(client) = resolver
                    .resolve(client_id)
                    .await
                    .map_err(|_| McpOAuthError::Unavailable)?
                else {
                    return Err(McpOAuthError::InvalidClient);
                };
                if client.client_id != client_id {
                    return Err(McpOAuthError::InvalidClient);
                }
                validate_client_metadata(&client)?;
                (client, McpClientRegistrationMethod::MetadataDocument)
            }
        };
        let redirect_uri = match redirect_uri {
            Some(value)
                if valid_redirect_uri(value)
                    && client
                        .redirect_uris
                        .iter()
                        .any(|registered| redirect_uri_matches(registered, value)) =>
            {
                value.to_owned()
            }
            None if client.redirect_uris.len() == 1 => client.redirect_uris[0].clone(),
            _ => return Err(McpOAuthError::InvalidRedirectUri),
        };
        Ok((
            McpAuthorizationClient {
                client,
                redirect_uri,
            },
            registration_method,
        ))
    }

    pub async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: Option<String>,
        resource_uri: String,
        verifier: String,
    ) -> Result<McpIssuedTokenPair, McpOAuthError> {
        if !resource_uri_matches(&self.resource_uri, &resource_uri) {
            return Err(McpOAuthError::InvalidTarget);
        }
        if redirect_uri.is_none() {
            return Err(McpOAuthError::InvalidGrant);
        }
        if !valid_pkce_verifier(&verifier) {
            return Err(McpOAuthError::InvalidGrant);
        }
        let expected_challenge = pkce_s256(&verifier);
        let now = self.clock.now();
        let access_token = self.random.opaque_token();
        let refresh_token = self.random.opaque_token();
        let Some(grant) = self
            .repository
            .exchange_authorization_code(
                McpAuthorizationCodeExchange {
                    code,
                    client_id,
                    redirect_uri,
                    resource_uri: self.resource_uri.clone(),
                    code_challenge: expected_challenge,
                    access_token: access_token.clone(),
                    refresh_token: refresh_token.clone(),
                    access_expires_at: UnixMillis::new(
                        now.get() + (Self::ACCESS_TOKEN_SECONDS * 1_000) as i64,
                    ),
                    refresh_expires_at: UnixMillis::new(
                        now.get() + (Self::REFRESH_TOKEN_SECONDS * 1_000) as i64,
                    ),
                },
                now,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Err(McpOAuthError::InvalidGrant);
        };
        Ok(McpIssuedTokenPair {
            access_token,
            refresh_token,
            access_expires_in_seconds: Self::ACCESS_TOKEN_SECONDS,
            scope: grant.scopes.join(" "),
        })
    }

    pub async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
        scopes: Option<Vec<String>>,
    ) -> Result<McpIssuedTokenPair, McpOAuthError> {
        if !resource_uri_matches(&self.resource_uri, &resource_uri) {
            return Err(McpOAuthError::InvalidTarget);
        }
        if scopes
            .as_ref()
            .is_some_and(|requested| !valid_mcp_scopes(requested))
        {
            return Err(McpOAuthError::InvalidScope);
        }
        let now = self.clock.now();
        let access_token = self.random.opaque_token();
        let next_refresh_token = self.random.opaque_token();
        let outcome = self
            .repository
            .rotate_refresh_token(
                McpRefreshTokenRotation {
                    refresh_token,
                    client_id,
                    resource_uri: self.resource_uri.clone(),
                    requested_scopes: scopes.map(|value| canonical_scopes(&value)),
                    new_access_token: access_token.clone(),
                    new_refresh_token: next_refresh_token.clone(),
                    access_expires_at: UnixMillis::new(
                        now.get() + (Self::ACCESS_TOKEN_SECONDS * 1_000) as i64,
                    ),
                    refresh_expires_at: UnixMillis::new(
                        now.get() + (Self::REFRESH_TOKEN_SECONDS * 1_000) as i64,
                    ),
                },
                now,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        let access_scopes = match outcome {
            McpRefreshTokenRotationOutcome::Rotated {
                grant: _,
                access_scopes,
            } => access_scopes,
            McpRefreshTokenRotationOutcome::InvalidToken => {
                return Err(McpOAuthError::InvalidGrant);
            }
            McpRefreshTokenRotationOutcome::InvalidScope => {
                return Err(McpOAuthError::InvalidScope);
            }
        };
        Ok(McpIssuedTokenPair {
            access_token,
            refresh_token: next_refresh_token,
            access_expires_in_seconds: Self::ACCESS_TOKEN_SECONDS,
            scope: access_scopes.join(" "),
        })
    }

    pub async fn authenticate(
        &self,
        token: &str,
        resource_uri: &str,
    ) -> Result<Option<marginalis_domain::McpAuthenticatedActor>, McpOAuthError> {
        if !resource_uri_matches(&self.resource_uri, resource_uri) {
            return Ok(None);
        }
        let Some(authenticated) = self
            .repository
            .authenticate_access_token(token, &self.resource_uri, self.clock.now())
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Ok(None);
        };
        Ok(Some(authenticated))
    }

    pub async fn revoke(&self, actor: &Actor, client_id: &str) -> Result<(), McpOAuthError> {
        self.repository
            .revoke_client_tokens(actor.issuer(), actor.subject(), client_id, self.clock.now())
            .await
            .map_err(|_| McpOAuthError::Unavailable)
    }

    pub async fn revoke_token(&self, token: &str, client_id: &str) -> Result<(), McpOAuthError> {
        if token.is_empty() || client_id.is_empty() {
            return Err(McpOAuthError::InvalidRequest);
        }
        self.repository
            .revoke_token(token, client_id, self.clock.now())
            .await
            .map_err(|_| McpOAuthError::Unavailable)
    }
}

#[async_trait]
impl McpOAuthUseCases for McpOAuthApplication {
    async fn register_client(
        &self,
        client: marginalis_domain::McpOAuthClient,
    ) -> Result<(), McpOAuthUseCaseError> {
        McpOAuthApplication::register_client(self, client).await
    }
    async fn resolve_authorization_client(
        &self,
        client_id: String,
        redirect_uri: Option<String>,
    ) -> Result<McpAuthorizationClient, McpOAuthUseCaseError> {
        McpOAuthApplication::resolve_authorization_client(self, &client_id, redirect_uri.as_deref())
            .await
    }
    async fn validate_authorization_request(
        &self,
        request: McpAuthorizationRequest,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthUseCaseError> {
        McpOAuthApplication::validate_authorization_request(self, &request).await
    }
    async fn authorize(
        &self,
        actor: Actor,
        request: McpValidatedAuthorizationRequest,
    ) -> Result<String, McpOAuthUseCaseError> {
        McpOAuthApplication::authorize(self, actor, request).await
    }
    async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: Option<String>,
        resource_uri: String,
        verifier: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        McpOAuthApplication::exchange_authorization_code(
            self,
            code,
            client_id,
            redirect_uri,
            resource_uri,
            verifier,
        )
        .await
    }
    async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
        scopes: Option<Vec<String>>,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        McpOAuthApplication::refresh_access_token(
            self,
            refresh_token,
            client_id,
            resource_uri,
            scopes,
        )
        .await
    }
    async fn authenticate(
        &self,
        token: String,
        resource_uri: String,
    ) -> Result<Option<marginalis_domain::McpAuthenticatedActor>, McpOAuthUseCaseError> {
        McpOAuthApplication::authenticate(self, &token, &resource_uri).await
    }
    async fn revoke(&self, actor: Actor, client_id: String) -> Result<(), McpOAuthUseCaseError> {
        McpOAuthApplication::revoke(self, &actor, &client_id).await
    }
    async fn revoke_token(
        &self,
        token: String,
        client_id: String,
    ) -> Result<(), McpOAuthUseCaseError> {
        McpOAuthApplication::revoke_token(self, &token, &client_id).await
    }
}

fn pkce_s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn valid_pkce_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_pkce_verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn valid_mcp_scopes(scopes: &[String]) -> bool {
    !scopes.is_empty()
        && scopes.iter().all(|scope| {
            matches!(
                scope.as_str(),
                "notes:read" | "notes:write" | "notes:delete"
            )
        })
}

fn canonical_scopes(scopes: &[String]) -> Vec<String> {
    ["notes:read", "notes:write", "notes:delete"]
        .into_iter()
        .filter(|candidate| scopes.iter().any(|scope| scope == candidate))
        .map(str::to_owned)
        .collect()
}

fn valid_redirect_uri(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if url.host().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    if url.scheme() == "https" {
        return true;
    }
    if url.scheme() != "http" {
        return false;
    }
    match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

fn validate_client_metadata(client: &McpOAuthClient) -> Result<(), McpOAuthError> {
    if client.client_id.is_empty()
        || client.client_id.len() > 2_048
        || client.display_name.trim().is_empty()
        || client.redirect_uris.is_empty()
        || client.display_name.len() > 128
        || client.redirect_uris.len() > 8
        || client.display_name.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return Err(McpOAuthError::InvalidRequest);
    }
    if !client
        .redirect_uris
        .iter()
        .all(|uri| uri.len() <= 2_048 && valid_redirect_uri(uri))
    {
        return Err(McpOAuthError::InvalidRedirectUri);
    }
    Ok(())
}

fn valid_client_metadata_document_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    let Some(authority_end) = value
        .strip_prefix("https://")
        .and_then(|remainder| remainder.find('/').map(|index| index + "https://".len()))
    else {
        return false;
    };
    let raw_path = value[authority_end..]
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    let has_dot_segment = raw_path.split('/').any(|segment| {
        matches!(
            segment.to_ascii_lowercase().as_str(),
            "." | ".." | "%2e" | ".%2e" | "%2e." | "%2e%2e"
        )
    });
    value.len() <= 2_048
        && url.scheme() == "https"
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && !raw_path.is_empty()
        && !has_dot_segment
}

fn redirect_uri_matches(registered: &str, requested: &str) -> bool {
    if registered == requested {
        return true;
    }
    let (Ok(mut registered), Ok(mut requested)) = (Url::parse(registered), Url::parse(requested))
    else {
        return false;
    };
    if registered.scheme() != "http"
        || requested.scheme() != "http"
        || !is_loopback_host(&registered)
        || !is_loopback_host(&requested)
        || registered.host() != requested.host()
    {
        return false;
    }
    let _ = registered.set_port(None);
    let _ = requested.set_port(None);
    registered == requested
}

fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

fn resource_uri_matches(expected: &str, received: &str) -> bool {
    match (Url::parse(expected), Url::parse(received)) {
        (Ok(expected), Ok(received)) => expected == received,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_redirects_require_https_or_a_loopback_host() {
        for valid in [
            "https://chatgpt.com/connector/oauth/callback",
            "http://localhost/callback",
            "http://localhost:48123/callback",
            "http://127.0.0.1/callback",
            "http://127.0.0.1:48123/callback",
            "http://[::1]:48123/callback",
        ] {
            assert!(valid_redirect_uri(valid), "{valid}");
        }
        for invalid in [
            "http://localhost.example:48123/callback",
            "http://client.example.test/callback",
            "https://client.example.test/callback?next=other",
            "https://user@client.example.test/callback",
        ] {
            assert!(!valid_redirect_uri(invalid), "{invalid}");
        }
        assert!(redirect_uri_matches(
            "http://127.0.0.1/callback",
            "http://127.0.0.1:49152/callback"
        ));
        assert!(!redirect_uri_matches(
            "http://127.0.0.1/callback",
            "http://127.0.0.1:49152/other"
        ));
        assert!(resource_uri_matches(
            "HTTPS://Notes.Example.Test/mcp",
            "https://notes.example.test/mcp"
        ));
    }

    #[test]
    fn client_metadata_document_url_has_a_safe_https_path() {
        assert!(valid_client_metadata_document_url(
            "https://client.example/oauth/metadata.json"
        ));
        for invalid in [
            "https://client.example",
            "https://user@client.example/metadata.json",
            "https://client.example/a/../metadata.json",
            "https://client.example/a/%2e%2e/metadata.json",
            "https://client.example/metadata.json?version=1",
            "https://client.example/metadata.json#client",
        ] {
            assert!(!valid_client_metadata_document_url(invalid), "{invalid}");
        }
    }
}
