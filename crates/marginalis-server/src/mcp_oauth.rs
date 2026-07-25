//! MCP用OAuth Authorization Serverのuse case。

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    Clock, McpAuthorizationClient, McpAuthorizationCodeExchange, McpAuthorizationRequest,
    McpOAuthUseCaseError, McpOAuthUseCases, McpRefreshTokenRotation,
    McpRefreshTokenRotationOutcome, McpTokenPair, McpValidatedAuthorizationRequest, Random,
};
use marginalis_domain::{Actor, UnixMillis};
use marginalis_sqlite::SqliteDatabase;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{SystemClock, SystemRandom};

type McpIssuedTokenPair = McpTokenPair;
type McpOAuthError = McpOAuthUseCaseError;

/// v0.3 SQLite schemaとKanidm主体を使うMCP OAuth service。
#[derive(Clone)]
pub struct ServerMcpOAuthService {
    database: SqliteDatabase,
    resource_uri: String,
}

impl ServerMcpOAuthService {
    pub const ACCESS_TOKEN_SECONDS: u64 = 60 * 60;
    pub const REFRESH_TOKEN_SECONDS: u64 = 30 * 24 * 60 * 60;
    const MAX_DYNAMIC_CLIENTS: i64 = 1_000;

    pub fn new(database: SqliteDatabase, resource_uri: String) -> Self {
        Self {
            database,
            resource_uri,
        }
    }

    pub async fn register_client(
        &self,
        client: marginalis_domain::McpOAuthClient,
    ) -> Result<(), McpOAuthError> {
        if client.client_id.is_empty()
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
        let now = SystemClock.now();
        let registered = self
            .database
            .register_mcp_client_bounded(&client, now, Self::MAX_DYNAMIC_CLIENTS)
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        if !registered {
            return Err(McpOAuthError::InvalidRequest);
        }
        Ok(())
    }

    pub async fn authorize(
        &self,
        actor: Actor,
        request: McpValidatedAuthorizationRequest,
    ) -> Result<String, McpOAuthError> {
        let code = SystemRandom.opaque_token();
        let grant = marginalis_domain::McpAuthorizationGrant {
            actor,
            client_id: request.client.client_id,
            redirect_uri: request.redirect_uri,
            resource_uri: request.resource_uri,
            scopes: request.scopes,
        };
        self.database
            .issue_mcp_authorization_code(
                &code,
                &grant,
                &request.code_challenge,
                UnixMillis::new(SystemClock.now().get() + 5 * 60 * 1_000),
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        Ok(code)
    }

    pub async fn validate_authorization_request(
        &self,
        request: &McpAuthorizationRequest,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthError> {
        let resolved = self
            .resolve_authorization_client(&request.client_id, request.redirect_uri.as_deref())
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
        let Some(client) = self
            .database
            .mcp_client(client_id)
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Err(McpOAuthError::InvalidClient);
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
        Ok(McpAuthorizationClient {
            client,
            redirect_uri,
        })
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
        if !valid_pkce_verifier(&verifier) {
            return Err(McpOAuthError::InvalidGrant);
        }
        let expected_challenge = pkce_s256(&verifier);
        let now = SystemClock.now();
        let access_token = SystemRandom.opaque_token();
        let refresh_token = SystemRandom.opaque_token();
        let Some(grant) = self
            .database
            .exchange_mcp_authorization_code(
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
        let now = SystemClock.now();
        let access_token = SystemRandom.opaque_token();
        let next_refresh_token = SystemRandom.opaque_token();
        let outcome = self
            .database
            .rotate_mcp_refresh_token(
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
            .database
            .authenticate_mcp_access_token(token, &self.resource_uri, SystemClock.now())
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Ok(None);
        };
        Ok(Some(authenticated))
    }

    pub async fn revoke(&self, actor: &Actor, client_id: &str) -> Result<(), McpOAuthError> {
        self.database
            .revoke_mcp_client_tokens(&actor.issuer, &actor.subject, client_id, SystemClock.now())
            .await
            .map_err(|_| McpOAuthError::Unavailable)
    }
}

#[async_trait]
impl McpOAuthUseCases for ServerMcpOAuthService {
    async fn register_client(
        &self,
        client: marginalis_domain::McpOAuthClient,
    ) -> Result<(), McpOAuthUseCaseError> {
        ServerMcpOAuthService::register_client(self, client).await
    }
    async fn resolve_authorization_client(
        &self,
        client_id: String,
        redirect_uri: Option<String>,
    ) -> Result<McpAuthorizationClient, McpOAuthUseCaseError> {
        ServerMcpOAuthService::resolve_authorization_client(
            self,
            &client_id,
            redirect_uri.as_deref(),
        )
        .await
    }
    async fn validate_authorization_request(
        &self,
        request: McpAuthorizationRequest,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthUseCaseError> {
        ServerMcpOAuthService::validate_authorization_request(self, &request).await
    }
    async fn authorize(
        &self,
        actor: Actor,
        request: McpValidatedAuthorizationRequest,
    ) -> Result<String, McpOAuthUseCaseError> {
        ServerMcpOAuthService::authorize(self, actor, request).await
    }
    async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: Option<String>,
        resource_uri: String,
        verifier: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        ServerMcpOAuthService::exchange_authorization_code(
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
        ServerMcpOAuthService::refresh_access_token(
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
        ServerMcpOAuthService::authenticate(self, &token, &resource_uri).await
    }
    async fn revoke(&self, actor: Actor, client_id: String) -> Result<(), McpOAuthUseCaseError> {
        ServerMcpOAuthService::revoke(self, &actor, &client_id).await
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

    #[tokio::test]
    async fn mcp_oauth_rotates_tokens_and_honors_revocation() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let resource_uri = "https://notes.example.test/mcp".to_owned();
        let service = ServerMcpOAuthService::new(database, resource_uri.clone());
        let client = marginalis_domain::McpOAuthClient {
            client_id: "https://client.example.test/mcp.json".into(),
            display_name: "Client".into(),
            redirect_uris: vec!["https://client.example.test/callback".into()],
        };
        service
            .register_client(client.clone())
            .await
            .expect("client");
        let actor = Actor {
            issuer: "https://id.example.test".into(),
            subject: "alice".into(),
            is_administrator: false,
        };
        let verifier = "v3-pkce-verifier-which-is-at-least-forty-three-characters".to_owned();
        assert_eq!(
            service
                .validate_authorization_request(&McpAuthorizationRequest {
                    client_id: client.client_id.clone(),
                    redirect_uri: Some(client.redirect_uris[0].clone()),
                    resource_uri: "https://other.example.test/mcp".into(),
                    scopes: vec!["notes:read".into()],
                    code_challenge: pkce_s256(&verifier),
                })
                .await,
            Err(McpOAuthError::InvalidTarget)
        );
        assert_eq!(
            service
                .validate_authorization_request(&McpAuthorizationRequest {
                    client_id: client.client_id.clone(),
                    redirect_uri: Some(client.redirect_uris[0].clone()),
                    resource_uri: resource_uri.clone(),
                    scopes: vec!["notes:read".into()],
                    code_challenge: "short".into(),
                })
                .await,
            Err(McpOAuthError::InvalidRequest)
        );
        let validated = service
            .validate_authorization_request(&McpAuthorizationRequest {
                client_id: client.client_id.clone(),
                redirect_uri: None,
                resource_uri: resource_uri.clone(),
                scopes: Vec::new(),
                code_challenge: pkce_s256(&verifier),
            })
            .await
            .expect("valid authorization request");
        assert_eq!(validated.redirect_uri, client.redirect_uris[0]);
        assert_eq!(validated.scopes, vec!["notes:read"]);
        let code = service
            .authorize(actor.clone(), validated)
            .await
            .expect("authorize");
        let tokens = service
            .exchange_authorization_code(
                code,
                client.client_id.clone(),
                None,
                resource_uri.clone(),
                verifier.clone(),
            )
            .await
            .expect("exchange");
        assert!(
            service
                .authenticate(&tokens.access_token, &resource_uri)
                .await
                .expect("authenticate")
                .is_some()
        );
        let original_refresh_token = tokens.refresh_token.clone();
        let rotated = service
            .refresh_access_token(
                tokens.refresh_token,
                client.client_id.clone(),
                resource_uri.clone(),
                None,
            )
            .await
            .expect("refresh");
        assert!(matches!(
            service
                .refresh_access_token(
                    original_refresh_token,
                    client.client_id.clone(),
                    resource_uri.clone(),
                    None,
                )
                .await,
            Err(McpOAuthError::InvalidGrant)
        ));
        assert!(
            service
                .authenticate(&rotated.access_token, &resource_uri)
                .await
                .expect("replayed token family")
                .is_none()
        );

        let replacement_request = service
            .validate_authorization_request(&McpAuthorizationRequest {
                client_id: client.client_id.clone(),
                redirect_uri: Some(client.redirect_uris[0].clone()),
                resource_uri: resource_uri.clone(),
                scopes: vec!["notes:read".into()],
                code_challenge: pkce_s256(&verifier),
            })
            .await
            .expect("valid replacement request");
        let replacement_code = service
            .authorize(actor.clone(), replacement_request)
            .await
            .expect("replacement authorization");
        let replacement = service
            .exchange_authorization_code(
                replacement_code,
                client.client_id.clone(),
                Some(client.redirect_uris[0].clone()),
                resource_uri.clone(),
                verifier,
            )
            .await
            .expect("replacement exchange");
        service
            .revoke(&actor, &client.client_id)
            .await
            .expect("revoke");
        assert!(
            service
                .authenticate(&replacement.access_token, &resource_uri)
                .await
                .expect("revoked")
                .is_none()
        );
    }
}
