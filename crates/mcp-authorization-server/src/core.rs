use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    AuthenticatedPrincipal, AuthorizationClient, AuthorizationClientError,
    AuthorizationCodeExchange, AuthorizationError, AuthorizationGrant, AuthorizationRequest,
    Client, ClientRegistrationMethod, Principal, RefreshTokenRotation, RefreshTokenRotationOutcome,
    RegisteredClient, ResolvedRedirectUri, ResourcePolicy, ScopeCeilings, Timestamp, TokenPair,
    ValidatedAuthorizationRequest, canonical_scopes, pkce_s256, redirect_uri_matches,
    valid_client_metadata_document_url, valid_pkce_challenge, valid_pkce_verifier,
    valid_redirect_uri, validate_client_metadata,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientMetadataResolverError;

/// client、code、token familyを原子的に保存する外向きport。
#[async_trait]
pub trait Repository: Send + Sync {
    async fn register_client_bounded(
        &self,
        client: &Client,
        now: Timestamp,
        maximum_clients: i64,
    ) -> Result<bool, RepositoryError>;

    async fn client(&self, client_id: &str) -> Result<Option<RegisteredClient>, RepositoryError>;

    /// 認可codeを保存する。`client`は同じtransactionで登録し、外部keyを満たす。
    async fn issue_authorization_code(
        &self,
        code: &str,
        client: &RegisteredClient,
        grant: &AuthorizationGrant,
        code_challenge: &str,
        expires_at: Timestamp,
        now: Timestamp,
    ) -> Result<(), RepositoryError>;

    /// 認可codeの一回消費とtoken pair発行を同じtransactionで行う。
    /// `expires_at <= now`のcodeは無効として扱う。
    async fn exchange_authorization_code(
        &self,
        exchange: AuthorizationCodeExchange,
        now: Timestamp,
    ) -> Result<Option<AuthorizationGrant>, RepositoryError>;

    /// refresh tokenの一回消費、再利用検知、次のtoken pair発行を原子的に行う。
    /// `expires_at <= now`のtokenは無効として扱う。
    async fn rotate_refresh_token(
        &self,
        rotation: RefreshTokenRotation,
        now: Timestamp,
    ) -> Result<RefreshTokenRotationOutcome, RepositoryError>;

    /// resourceが一致し、`expires_at > now`であるaccess tokenだけを認証する。
    async fn authenticate_access_token(
        &self,
        token: &str,
        resource_uri: &str,
        now: Timestamp,
    ) -> Result<Option<AuthenticatedPrincipal>, RepositoryError>;

    async fn revoke_client_tokens(
        &self,
        issuer: &str,
        subject: &str,
        client_id: &str,
        now: Timestamp,
    ) -> Result<(), RepositoryError>;

    async fn revoke_token(
        &self,
        token: &str,
        client_id: &str,
        now: Timestamp,
    ) -> Result<(), RepositoryError>;
}

/// HTTPSのClient ID Metadata Documentを取得する外向きport。
#[async_trait]
pub trait ClientMetadataResolver: Send + Sync {
    async fn resolve(&self, client_id: &str)
    -> Result<Option<Client>, ClientMetadataResolverError>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// 実装は暗号学的に安全な乱数を使う。試験実装は決定的な値を供給できる。
pub trait Random: Send + Sync {
    fn opaque_token(&self) -> String;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizationServerConfig {
    pub access_token_seconds: u64,
    pub refresh_token_seconds: u64,
    pub authorization_code_seconds: u64,
    pub maximum_dynamic_clients: i64,
}

impl AuthorizationServerConfig {
    pub const fn new(
        access_token_seconds: u64,
        refresh_token_seconds: u64,
        authorization_code_seconds: u64,
        maximum_dynamic_clients: i64,
    ) -> Option<Self> {
        if access_token_seconds == 0
            || refresh_token_seconds == 0
            || authorization_code_seconds == 0
            || maximum_dynamic_clients <= 0
        {
            return None;
        }
        Some(Self {
            access_token_seconds,
            refresh_token_seconds,
            authorization_code_seconds,
            maximum_dynamic_clients,
        })
    }
}

/// MCPに必要なOAuth状態遷移を実行する製品非依存の中核。
pub struct AuthorizationServer {
    repository: Arc<dyn Repository>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
    resource_policy: ResourcePolicy,
    config: AuthorizationServerConfig,
    client_metadata_resolver: Option<Arc<dyn ClientMetadataResolver>>,
}

impl AuthorizationServer {
    pub fn new(
        repository: Arc<dyn Repository>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
        resource_policy: ResourcePolicy,
        config: AuthorizationServerConfig,
    ) -> Self {
        Self {
            repository,
            clock,
            random,
            resource_policy,
            config,
            client_metadata_resolver: None,
        }
    }

    pub fn with_client_metadata_resolver(
        mut self,
        resolver: Arc<dyn ClientMetadataResolver>,
    ) -> Self {
        self.client_metadata_resolver = Some(resolver);
        self
    }

    pub async fn register_client(&self, client: Client) -> Result<(), AuthorizationError> {
        map_client_metadata_error(validate_client_metadata(&client))?;
        let registered = self
            .repository
            .register_client_bounded(
                &client,
                self.clock.now(),
                self.config.maximum_dynamic_clients,
            )
            .await
            .map_err(|_| AuthorizationError::Unavailable)?;
        if !registered {
            return Err(AuthorizationError::Capacity);
        }
        Ok(())
    }

    pub async fn authorize(
        &self,
        principal: Principal,
        mut request: ValidatedAuthorizationRequest,
        ceilings: &ScopeCeilings,
    ) -> Result<String, AuthorizationError> {
        request.scopes = self
            .resource_policy
            .eligible_scopes(&request.scopes, ceilings)
            .ok_or(AuthorizationError::InvalidScope)?;
        let code = self.random.opaque_token();
        let registered_client = RegisteredClient {
            client: request.client,
            registration_method: request.registration_method,
        };
        let grant = AuthorizationGrant {
            principal,
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
                expires_at(now, self.config.authorization_code_seconds),
                now,
            )
            .await
            .map_err(|_| AuthorizationError::Unavailable)?;
        Ok(code)
    }

    pub fn scope_ceilings(
        &self,
        principal: Vec<String>,
        client: Vec<String>,
    ) -> Result<ScopeCeilings, AuthorizationError> {
        self.resource_policy
            .scope_ceilings(principal, client)
            .map_err(|_| AuthorizationError::InvalidScope)
    }

    pub fn supported_scopes(&self) -> &[String] {
        self.resource_policy.supported_scopes()
    }

    /// metadata、challenge、認可、token検証で共有するresource policyの正本。
    pub const fn resource_policy(&self) -> &ResourcePolicy {
        &self.resource_policy
    }

    pub async fn validate_authorization_request(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<ValidatedAuthorizationRequest, AuthorizationError> {
        let resolved = self
            .resolve_authorization_client(&request.client_id, request.redirect_uri.as_deref())
            .await?;
        self.validate_resolved_authorization_request(request, resolved)
    }

    pub fn validate_resolved_authorization_request(
        &self,
        request: &AuthorizationRequest,
        resolved: AuthorizationClient,
    ) -> Result<ValidatedAuthorizationRequest, AuthorizationError> {
        if request.client_id != resolved.client.client_id {
            return Err(AuthorizationError::InvalidClient);
        }
        if request
            .redirect_uri
            .as_ref()
            .is_some_and(|redirect_uri| redirect_uri != &resolved.redirect_uri)
        {
            return Err(AuthorizationError::InvalidRedirectUri);
        }
        let client = resolved.client;
        let registration_method = resolved.registration_method;
        let redirect_uri = if request.redirect_uri.is_some() {
            ResolvedRedirectUri::Supplied(resolved.redirect_uri)
        } else {
            ResolvedRedirectUri::Inferred(resolved.redirect_uri)
        };
        if !self
            .resource_policy
            .resource_uri_matches(&request.resource_uri)
        {
            return Err(AuthorizationError::InvalidTarget);
        }
        let scopes = self
            .resource_policy
            .resolve_scopes(&request.scopes)
            .ok_or(AuthorizationError::InvalidScope)?;
        if !valid_pkce_challenge(&request.code_challenge) {
            return Err(AuthorizationError::InvalidRequest);
        }
        Ok(ValidatedAuthorizationRequest {
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
    ) -> Result<AuthorizationClient, AuthorizationError> {
        let stored = self
            .repository
            .client(client_id)
            .await
            .map_err(|_| AuthorizationError::Unavailable)?;
        let (client, registration_method) = match stored {
            Some(RegisteredClient {
                client,
                registration_method: ClientRegistrationMethod::Dynamic,
            }) => (client, ClientRegistrationMethod::Dynamic),
            Some(RegisteredClient {
                registration_method: ClientRegistrationMethod::MetadataDocument,
                ..
            })
            | None => {
                if !valid_client_metadata_document_url(client_id) {
                    return Err(AuthorizationError::InvalidClient);
                }
                let Some(resolver) = &self.client_metadata_resolver else {
                    return Err(AuthorizationError::InvalidClient);
                };
                let Some(client) = resolver
                    .resolve(client_id)
                    .await
                    .map_err(|_| AuthorizationError::Unavailable)?
                else {
                    return Err(AuthorizationError::InvalidClient);
                };
                if client.client_id != client_id {
                    return Err(AuthorizationError::InvalidClient);
                }
                map_client_metadata_error(validate_client_metadata(&client))?;
                (client, ClientRegistrationMethod::MetadataDocument)
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
            _ => return Err(AuthorizationError::InvalidRedirectUri),
        };
        Ok(AuthorizationClient {
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
    ) -> Result<TokenPair, AuthorizationError> {
        if !self.resource_policy.resource_uri_matches(&resource_uri) {
            return Err(AuthorizationError::InvalidTarget);
        }
        if !valid_pkce_verifier(&verifier) {
            return Err(AuthorizationError::InvalidGrant);
        }
        let now = self.clock.now();
        let access_token = self.random.opaque_token();
        let refresh_token = self.random.opaque_token();
        let Some(grant) = self
            .repository
            .exchange_authorization_code(
                AuthorizationCodeExchange {
                    code,
                    client_id,
                    redirect_uri,
                    resource_uri: self.resource_policy.uri().to_string(),
                    code_challenge: pkce_s256(&verifier),
                    access_token: access_token.clone(),
                    refresh_token: refresh_token.clone(),
                    access_expires_at: expires_at(now, self.config.access_token_seconds),
                    refresh_expires_at: expires_at(now, self.config.refresh_token_seconds),
                },
                now,
            )
            .await
            .map_err(|_| AuthorizationError::Unavailable)?
        else {
            return Err(AuthorizationError::InvalidGrant);
        };
        Ok(TokenPair {
            access_token,
            refresh_token,
            access_expires_in_seconds: self.config.access_token_seconds,
            scope: grant.scopes.join(" "),
        })
    }

    pub async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
        scopes: Option<Vec<String>>,
    ) -> Result<TokenPair, AuthorizationError> {
        if !self.resource_policy.resource_uri_matches(&resource_uri) {
            return Err(AuthorizationError::InvalidTarget);
        }
        if scopes
            .as_ref()
            .is_some_and(|requested| self.resource_policy.resolve_scopes(requested).is_none())
        {
            return Err(AuthorizationError::InvalidScope);
        }
        let now = self.clock.now();
        let access_token = self.random.opaque_token();
        let next_refresh_token = self.random.opaque_token();
        let outcome = self
            .repository
            .rotate_refresh_token(
                RefreshTokenRotation {
                    refresh_token,
                    client_id,
                    resource_uri: self.resource_policy.uri().to_string(),
                    requested_scopes: scopes.map(|value| {
                        canonical_scopes(&value, self.resource_policy.supported_scopes())
                    }),
                    new_access_token: access_token.clone(),
                    new_refresh_token: next_refresh_token.clone(),
                    access_expires_at: expires_at(now, self.config.access_token_seconds),
                    refresh_expires_at: expires_at(now, self.config.refresh_token_seconds),
                },
                now,
            )
            .await
            .map_err(|_| AuthorizationError::Unavailable)?;
        let access_scopes = match outcome {
            RefreshTokenRotationOutcome::Rotated { access_scopes } => access_scopes,
            RefreshTokenRotationOutcome::InvalidToken => {
                return Err(AuthorizationError::InvalidGrant);
            }
            RefreshTokenRotationOutcome::InvalidScope => {
                return Err(AuthorizationError::InvalidScope);
            }
        };
        Ok(TokenPair {
            access_token,
            refresh_token: next_refresh_token,
            access_expires_in_seconds: self.config.access_token_seconds,
            scope: access_scopes.join(" "),
        })
    }

    pub async fn authenticate(
        &self,
        token: &str,
        resource_uri: &str,
    ) -> Result<Option<AuthenticatedPrincipal>, AuthorizationError> {
        if !self.resource_policy.resource_uri_matches(resource_uri) {
            return Ok(None);
        }
        self.repository
            .authenticate_access_token(token, self.resource_policy.uri().as_str(), self.clock.now())
            .await
            .map_err(|_| AuthorizationError::Unavailable)
    }

    pub async fn revoke(
        &self,
        principal: &Principal,
        client_id: &str,
    ) -> Result<(), AuthorizationError> {
        self.repository
            .revoke_client_tokens(
                principal.issuer(),
                principal.subject(),
                client_id,
                self.clock.now(),
            )
            .await
            .map_err(|_| AuthorizationError::Unavailable)
    }

    pub async fn revoke_token(
        &self,
        token: &str,
        client_id: &str,
    ) -> Result<(), AuthorizationError> {
        if token.is_empty() || client_id.is_empty() {
            return Err(AuthorizationError::InvalidRequest);
        }
        self.repository
            .revoke_token(token, client_id, self.clock.now())
            .await
            .map_err(|_| AuthorizationError::Unavailable)
    }
}

fn expires_at(now: Timestamp, seconds: u64) -> Timestamp {
    let milliseconds = i64::try_from(seconds)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000))
        .and_then(|duration| now.get().checked_add(duration))
        .unwrap_or(i64::MAX);
    Timestamp::new(milliseconds)
}

fn map_client_metadata_error(
    result: Result<(), AuthorizationClientError>,
) -> Result<(), AuthorizationError> {
    result.map_err(|error| match error {
        AuthorizationClientError::InvalidMetadata => AuthorizationError::InvalidRequest,
        AuthorizationClientError::InvalidRedirectUri => AuthorizationError::InvalidRedirectUri,
    })
}
