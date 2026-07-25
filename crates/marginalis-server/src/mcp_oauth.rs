//! MCP用OAuth Authorization Serverのuse case。

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    Clock, McpAuthorizationRequest, McpOAuthUseCaseError, McpOAuthUseCases,
    McpRefreshTokenRotation, McpTokenPair, Random,
};
use marginalis_domain::{Actor, UnixMillis};
use marginalis_sqlite::SqliteDatabase;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{SystemClock, SystemRandom};

/// OAuth code exchangeの成功時だけtransportへ返すtoken pair。Debugを実装しない。
pub struct McpIssuedTokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_in_seconds: u64,
    pub scope: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpOAuthError {
    Rejected,
    Unavailable,
}

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
    const UNUSED_CLIENT_SECONDS: i64 = 24 * 60 * 60;

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
            || !client
                .redirect_uris
                .iter()
                .all(|uri| uri.len() <= 2_048 && valid_redirect_uri(uri))
        {
            return Err(McpOAuthError::Rejected);
        }
        let now = SystemClock.now();
        let registered = self
            .database
            .register_mcp_client_bounded(
                &client,
                now,
                UnixMillis::new(now.get() - Self::UNUSED_CLIENT_SECONDS * 1_000),
                Self::MAX_DYNAMIC_CLIENTS,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        if !registered {
            return Err(McpOAuthError::Rejected);
        }
        Ok(())
    }

    pub async fn authorize(
        &self,
        actor: Actor,
        request: McpAuthorizationRequest,
    ) -> Result<String, McpOAuthError> {
        self.validate_authorization_request(&request).await?;
        let code = SystemRandom.opaque_token();
        let grant = marginalis_domain::McpAuthorizationGrant {
            actor,
            client_id: request.client_id,
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
    ) -> Result<marginalis_domain::McpOAuthClient, McpOAuthError> {
        if request.resource_uri != self.resource_uri
            || !valid_mcp_scopes(&request.scopes)
            || !valid_pkce_challenge(&request.code_challenge)
            || !valid_redirect_uri(&request.redirect_uri)
        {
            return Err(McpOAuthError::Rejected);
        }
        let Some(client) = self
            .database
            .mcp_client(&request.client_id)
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Err(McpOAuthError::Rejected);
        };
        if !client.redirect_uris.contains(&request.redirect_uri) {
            return Err(McpOAuthError::Rejected);
        }
        Ok(client)
    }

    pub async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: String,
        resource_uri: String,
        verifier: String,
    ) -> Result<McpIssuedTokenPair, McpOAuthError> {
        if resource_uri != self.resource_uri || !valid_pkce_verifier(&verifier) {
            return Err(McpOAuthError::Rejected);
        }
        let expected_challenge = pkce_s256(&verifier);
        let now = SystemClock.now();
        let Some(grant) = self
            .database
            .consume_mcp_authorization_code(
                &code,
                &client_id,
                &redirect_uri,
                &resource_uri,
                &expected_challenge,
                now,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Err(McpOAuthError::Rejected);
        };
        self.issue_pair(grant, now).await
    }

    pub async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
    ) -> Result<McpIssuedTokenPair, McpOAuthError> {
        if resource_uri != self.resource_uri {
            return Err(McpOAuthError::Rejected);
        }
        let now = SystemClock.now();
        let access_token = SystemRandom.opaque_token();
        let next_refresh_token = SystemRandom.opaque_token();
        let Some(grant) = self
            .database
            .rotate_mcp_refresh_token(
                McpRefreshTokenRotation {
                    refresh_token,
                    client_id,
                    resource_uri,
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
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Err(McpOAuthError::Rejected);
        };
        Ok(McpIssuedTokenPair {
            access_token,
            refresh_token: next_refresh_token,
            access_expires_in_seconds: Self::ACCESS_TOKEN_SECONDS,
            scope: grant.scopes.join(" "),
        })
    }

    async fn issue_pair(
        &self,
        grant: marginalis_domain::McpAuthorizationGrant,
        now: UnixMillis,
    ) -> Result<McpIssuedTokenPair, McpOAuthError> {
        let access_token = SystemRandom.opaque_token();
        let refresh_token = SystemRandom.opaque_token();
        self.database
            .issue_mcp_token_pair(
                &access_token,
                &refresh_token,
                &grant,
                UnixMillis::new(now.get() + (Self::ACCESS_TOKEN_SECONDS * 1_000) as i64),
                UnixMillis::new(now.get() + (Self::REFRESH_TOKEN_SECONDS * 1_000) as i64),
                now,
            )
            .await
            .map_err(|_| McpOAuthError::Unavailable)?;
        Ok(McpIssuedTokenPair {
            access_token,
            refresh_token,
            access_expires_in_seconds: Self::ACCESS_TOKEN_SECONDS,
            scope: grant.scopes.join(" "),
        })
    }

    pub async fn authenticate(
        &self,
        token: &str,
        resource_uri: &str,
        scope: &str,
    ) -> Result<Option<marginalis_domain::McpAuthenticatedActor>, McpOAuthError> {
        let Some(authenticated) = self
            .database
            .authenticate_mcp_access_token(token, resource_uri, scope, SystemClock.now())
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

fn mcp_error(error: McpOAuthError) -> McpOAuthUseCaseError {
    match error {
        McpOAuthError::Rejected => McpOAuthUseCaseError::Rejected,
        McpOAuthError::Unavailable => McpOAuthUseCaseError::Unavailable,
    }
}

#[async_trait]
impl McpOAuthUseCases for ServerMcpOAuthService {
    async fn register_client(
        &self,
        client: marginalis_domain::McpOAuthClient,
    ) -> Result<(), McpOAuthUseCaseError> {
        self.register_client(client).await.map_err(mcp_error)
    }
    async fn validate_authorization_request(
        &self,
        request: McpAuthorizationRequest,
    ) -> Result<marginalis_domain::McpOAuthClient, McpOAuthUseCaseError> {
        self.validate_authorization_request(&request)
            .await
            .map_err(mcp_error)
    }
    async fn authorize(
        &self,
        actor: Actor,
        request: McpAuthorizationRequest,
    ) -> Result<String, McpOAuthUseCaseError> {
        self.authorize(actor, request).await.map_err(mcp_error)
    }
    async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: String,
        resource_uri: String,
        verifier: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        let pair = self
            .exchange_authorization_code(code, client_id, redirect_uri, resource_uri, verifier)
            .await
            .map_err(mcp_error)?;
        Ok(McpTokenPair {
            access_token: pair.access_token,
            refresh_token: pair.refresh_token,
            access_expires_in_seconds: pair.access_expires_in_seconds,
            scope: pair.scope,
        })
    }
    async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        let pair = self
            .refresh_access_token(refresh_token, client_id, resource_uri)
            .await
            .map_err(mcp_error)?;
        Ok(McpTokenPair {
            access_token: pair.access_token,
            refresh_token: pair.refresh_token,
            access_expires_in_seconds: pair.access_expires_in_seconds,
            scope: pair.scope,
        })
    }
    async fn authenticate(
        &self,
        token: String,
        resource_uri: String,
        scope: String,
    ) -> Result<Option<marginalis_domain::McpAuthenticatedActor>, McpOAuthUseCaseError> {
        self.authenticate(&token, &resource_uri, &scope)
            .await
            .map_err(mcp_error)
    }
    async fn revoke(&self, actor: Actor, client_id: String) -> Result<(), McpOAuthUseCaseError> {
        self.revoke(&actor, &client_id).await.map_err(mcp_error)
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
    if url.scheme() != "http" || url.port().is_none() {
        return false;
    }
    match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_redirects_require_https_or_an_explicit_loopback_port() {
        for valid in [
            "https://chatgpt.com/connector/oauth/callback",
            "http://localhost:48123/callback",
            "http://127.0.0.1:48123/callback",
            "http://[::1]:48123/callback",
        ] {
            assert!(valid_redirect_uri(valid), "{valid}");
        }
        for invalid in [
            "http://localhost/callback",
            "http://localhost.example:48123/callback",
            "http://127.0.0.1/callback",
            "http://client.example.test/callback",
            "https://client.example.test/callback?next=other",
            "https://user@client.example.test/callback",
        ] {
            assert!(!valid_redirect_uri(invalid), "{invalid}");
        }
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
                    redirect_uri: client.redirect_uris[0].clone(),
                    resource_uri: "https://other.example.test/mcp".into(),
                    scopes: vec!["notes:read".into()],
                    code_challenge: pkce_s256(&verifier),
                })
                .await,
            Err(McpOAuthError::Rejected)
        );
        assert_eq!(
            service
                .validate_authorization_request(&McpAuthorizationRequest {
                    client_id: client.client_id.clone(),
                    redirect_uri: client.redirect_uris[0].clone(),
                    resource_uri: resource_uri.clone(),
                    scopes: vec!["notes:read".into()],
                    code_challenge: "short".into(),
                })
                .await,
            Err(McpOAuthError::Rejected)
        );
        let code = service
            .authorize(
                actor.clone(),
                McpAuthorizationRequest {
                    client_id: client.client_id.clone(),
                    redirect_uri: client.redirect_uris[0].clone(),
                    resource_uri: resource_uri.clone(),
                    scopes: vec!["notes:read".into()],
                    code_challenge: pkce_s256(&verifier),
                },
            )
            .await
            .expect("authorize");
        let tokens = service
            .exchange_authorization_code(
                code,
                client.client_id.clone(),
                client.redirect_uris[0].clone(),
                resource_uri.clone(),
                verifier.clone(),
            )
            .await
            .expect("exchange");
        assert!(
            service
                .authenticate(&tokens.access_token, &resource_uri, "notes:read")
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
            )
            .await
            .expect("refresh");
        assert!(matches!(
            service
                .refresh_access_token(
                    original_refresh_token,
                    client.client_id.clone(),
                    resource_uri.clone(),
                )
                .await,
            Err(McpOAuthError::Rejected)
        ));
        assert!(
            service
                .authenticate(&rotated.access_token, &resource_uri, "notes:read")
                .await
                .expect("replayed token family")
                .is_none()
        );

        let replacement_code = service
            .authorize(
                actor.clone(),
                McpAuthorizationRequest {
                    client_id: client.client_id.clone(),
                    redirect_uri: client.redirect_uris[0].clone(),
                    resource_uri: resource_uri.clone(),
                    scopes: vec!["notes:read".into()],
                    code_challenge: pkce_s256(&verifier),
                },
            )
            .await
            .expect("replacement authorization");
        let replacement = service
            .exchange_authorization_code(
                replacement_code,
                client.client_id.clone(),
                client.redirect_uris[0].clone(),
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
                .authenticate(&replacement.access_token, &resource_uri, "notes:read")
                .await
                .expect("revoked")
                .is_none()
        );
    }
}
