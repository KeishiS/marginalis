//! MCP用OAuth Authorization Serverの業務処理と永続化port。

use async_trait::async_trait;
use marginalis_domain::{Actor, UnixMillis};
use mcp_authorization_server::{
    AuthenticatedPrincipal, AuthorizationClientError, Principal, ResourcePolicy, canonical_scopes,
    pkce_s256, redirect_uri_matches, valid_client_metadata_document_url, valid_pkce_challenge,
    valid_pkce_verifier, valid_redirect_uri, validate_client_metadata,
};
use std::sync::Arc;

use crate::{
    Clock, McpAuthenticatedActor, McpAuthorizationClient, McpAuthorizationCodeExchange,
    McpAuthorizationGrant, McpAuthorizationRequest, McpClientRegistrationMethod, McpOAuthClient,
    McpOAuthUseCaseError, McpOAuthUseCases, McpRefreshTokenRotation,
    McpRefreshTokenRotationOutcome, McpRegisteredOAuthClient, McpResolvedRedirectUri, McpTokenPair,
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
        grant: &McpAuthorizationGrant,
        code_challenge: &str,
        expires_at: UnixMillis,
        now: UnixMillis,
    ) -> Result<(), McpOAuthRepositoryError>;
    async fn exchange_authorization_code(
        &self,
        exchange: McpAuthorizationCodeExchange,
        now: UnixMillis,
    ) -> Result<Option<McpAuthorizationGrant>, McpOAuthRepositoryError>;
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
    ) -> Result<Option<AuthenticatedPrincipal>, McpOAuthRepositoryError>;
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
    resource_policy: ResourcePolicy,
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
        resource_policy: ResourcePolicy,
    ) -> Self {
        Self {
            repository,
            clock,
            random,
            resource_policy,
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

    pub async fn register_client(&self, client: McpOAuthClient) -> Result<(), McpOAuthError> {
        map_client_metadata_error(validate_client_metadata(&client))?;
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
        let grant = McpAuthorizationGrant {
            principal: Principal::new(actor.issuer().into(), actor.subject().into()),
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
        let resolved = self
            .resolve_authorization_client(&request.client_id, request.redirect_uri.as_deref())
            .await?;
        self.validate_resolved_authorization_request(request, resolved)
    }

    /// 同じ要求から解決した`resolved`を使い、clientを再取得せず残りの項目を検証する。
    pub fn validate_resolved_authorization_request(
        &self,
        request: &McpAuthorizationRequest,
        resolved: McpAuthorizationClient,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthError> {
        if request.client_id != resolved.client.client_id {
            return Err(McpOAuthError::InvalidClient);
        }
        if request
            .redirect_uri
            .as_ref()
            .is_some_and(|redirect_uri| redirect_uri != &resolved.redirect_uri)
        {
            return Err(McpOAuthError::InvalidRedirectUri);
        }
        let client = resolved.client;
        let registration_method = resolved.registration_method;
        let redirect_uri = if request.redirect_uri.is_some() {
            McpResolvedRedirectUri::Supplied(resolved.redirect_uri)
        } else {
            McpResolvedRedirectUri::Inferred(resolved.redirect_uri)
        };
        if !self
            .resource_policy
            .resource_uri_matches(&request.resource_uri)
        {
            return Err(McpOAuthError::InvalidTarget);
        }
        let scopes = self
            .resource_policy
            .resolve_scopes(&request.scopes)
            .ok_or(McpOAuthError::InvalidScope)?;
        if !valid_pkce_challenge(&request.code_challenge) {
            return Err(McpOAuthError::InvalidRequest);
        }
        Ok(McpValidatedAuthorizationRequest {
            client,
            registration_method,
            redirect_uri,
            resource_uri: self.resource_policy.uri().to_string(),
            scopes,
            code_challenge: request.code_challenge.clone(),
        })
    }

    pub async fn resolve_authorization_client(
        &self,
        client_id: &str,
        redirect_uri: Option<&str>,
    ) -> Result<McpAuthorizationClient, McpOAuthError> {
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
                map_client_metadata_error(validate_client_metadata(&client))?;
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
        Ok(McpAuthorizationClient {
            client,
            registration_method,
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
        if !self.resource_policy.resource_uri_matches(&resource_uri) {
            return Err(McpOAuthError::InvalidTarget);
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
                    resource_uri: self.resource_policy.uri().to_string(),
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
        if !self.resource_policy.resource_uri_matches(&resource_uri) {
            return Err(McpOAuthError::InvalidTarget);
        }
        if scopes
            .as_ref()
            .is_some_and(|requested| self.resource_policy.resolve_scopes(requested).is_none())
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
                    resource_uri: self.resource_policy.uri().to_string(),
                    requested_scopes: scopes.map(|value| {
                        canonical_scopes(&value, self.resource_policy.supported_scopes())
                    }),
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
            McpRefreshTokenRotationOutcome::Rotated { access_scopes } => access_scopes,
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
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthError> {
        if !self.resource_policy.resource_uri_matches(resource_uri) {
            return Ok(None);
        }
        let Some(authenticated) = self
            .repository
            .authenticate_access_token(token, self.resource_policy.uri().as_str(), self.clock.now())
            .await
            .map_err(|_| McpOAuthError::Unavailable)?
        else {
            return Ok(None);
        };
        let actor = Actor::try_new(
            authenticated.principal.issuer().into(),
            authenticated.principal.subject().into(),
        )
        .map_err(|_| McpOAuthError::Unavailable)?;
        Ok(Some(McpAuthenticatedActor {
            actor,
            scopes: authenticated.scopes,
        }))
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
    async fn register_client(&self, client: McpOAuthClient) -> Result<(), McpOAuthUseCaseError> {
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
    async fn validate_resolved_authorization_request(
        &self,
        request: McpAuthorizationRequest,
        resolved: McpAuthorizationClient,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthUseCaseError> {
        McpOAuthApplication::validate_resolved_authorization_request(self, &request, resolved)
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
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError> {
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

fn map_client_metadata_error(
    result: Result<(), AuthorizationClientError>,
) -> Result<(), McpOAuthError> {
    result.map_err(|error| match error {
        AuthorizationClientError::InvalidMetadata => McpOAuthError::InvalidRequest,
        AuthorizationClientError::InvalidRedirectUri => McpOAuthError::InvalidRedirectUri,
    })
}
