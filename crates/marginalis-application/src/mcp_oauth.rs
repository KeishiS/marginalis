//! 製品非依存のAuthorization ServerとMarginalisのidentity境界。

use std::sync::Arc;

use async_trait::async_trait;
use marginalis_domain::Actor;
use mcp_authorization_server::{
    AuthorizationServer, AuthorizationServerConfig, Clock as AuthorizationClock, Principal,
    Random as AuthorizationRandom, Timestamp,
};

use crate::{
    Clock, McpAuthenticatedActor, McpAuthorizationClient, McpAuthorizationRequest,
    McpClientAuthorization, McpClientMetadataResolver, McpOAuthClient, McpOAuthRepository,
    McpOAuthUseCaseError, McpOAuthUseCases, McpResourcePolicy, McpScopeCeilingRepository,
    McpScopeCeilingRepositoryError, McpScopeCeilingSetting, McpScopeCeilingUseCaseError,
    McpTokenPair, McpValidatedAuthorizationRequest, Random,
};

/// MarginalisのMCP Authorization Server設定。
const MCP_AUTHORIZATION_CONFIG: AuthorizationServerConfig =
    match AuthorizationServerConfig::new(60 * 60, 30 * 24 * 60 * 60, 5 * 60, 1_000) {
        Some(config) => config,
        None => panic!("MCP Authorization Server config must be valid"),
    };

struct ClockAdapter(Arc<dyn Clock>);

impl AuthorizationClock for ClockAdapter {
    fn now(&self) -> Timestamp {
        Timestamp::new(self.0.now().get())
    }
}

struct RandomAdapter(Arc<dyn Random>);

impl AuthorizationRandom for RandomAdapter {
    fn opaque_token(&self) -> String {
        self.0.opaque_token()
    }
}

/// 製品非依存の状態遷移へMarginalisの認証済み主体を渡すapplication service。
pub struct McpOAuthApplication {
    authorization_server: AuthorizationServer,
    scope_ceiling_repository: Arc<dyn McpScopeCeilingRepository>,
    clock: Arc<dyn Clock>,
}

impl McpOAuthApplication {
    pub fn new(
        repository: Arc<dyn McpOAuthRepository>,
        scope_ceiling_repository: Arc<dyn McpScopeCeilingRepository>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
        resource_policy: McpResourcePolicy,
    ) -> Self {
        let authorization_clock = clock.clone();
        Self {
            authorization_server: AuthorizationServer::new(
                repository,
                Arc::new(ClockAdapter(authorization_clock)),
                Arc::new(RandomAdapter(random)),
                resource_policy,
                MCP_AUTHORIZATION_CONFIG,
            ),
            scope_ceiling_repository,
            clock,
        }
    }

    pub fn with_client_metadata_resolver(
        mut self,
        resolver: Arc<dyn McpClientMetadataResolver>,
    ) -> Self {
        self.authorization_server = self
            .authorization_server
            .with_client_metadata_resolver(resolver);
        self
    }

    pub fn resource_policy(&self) -> &McpResourcePolicy {
        self.authorization_server.resource_policy()
    }

    pub async fn register_client(
        &self,
        client: McpOAuthClient,
    ) -> Result<(), McpOAuthUseCaseError> {
        self.authorization_server.register_client(client).await
    }

    pub async fn resolve_authorization_client(
        &self,
        client_id: &str,
        redirect_uri: Option<&str>,
    ) -> Result<McpAuthorizationClient, McpOAuthUseCaseError> {
        self.authorization_server
            .resolve_authorization_client(client_id, redirect_uri)
            .await
    }

    pub async fn validate_authorization_request(
        &self,
        request: &McpAuthorizationRequest,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthUseCaseError> {
        self.authorization_server
            .validate_authorization_request(request)
            .await
    }

    pub fn validate_resolved_authorization_request(
        &self,
        request: &McpAuthorizationRequest,
        resolved: McpAuthorizationClient,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthUseCaseError> {
        self.authorization_server
            .validate_resolved_authorization_request(request, resolved)
    }

    pub async fn authorize(
        &self,
        actor: Actor,
        request: McpValidatedAuthorizationRequest,
    ) -> Result<String, McpOAuthUseCaseError> {
        let stored = self
            .scope_ceiling_repository
            .scope_ceilings(&actor, &request.client.client_id)
            .await
            .map_err(|_| McpOAuthUseCaseError::Unavailable)?;
        let supported = self.authorization_server.supported_scopes();
        let ceilings = self
            .authorization_server
            .scope_ceilings(
                stored
                    .principal
                    .map_or_else(|| supported.to_vec(), |setting| setting.scopes),
                stored
                    .client
                    .map_or_else(|| supported.to_vec(), |setting| setting.scopes),
            )
            .map_err(|_| McpOAuthUseCaseError::Unavailable)?;
        self.authorization_server
            .authorize(principal(&actor), request, &ceilings)
            .await
    }

    pub async fn replace_principal_scope_ceiling(
        &self,
        actor: Actor,
        scopes: Vec<String>,
        expected_revision: i64,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingUseCaseError> {
        if expected_revision < 0 {
            return Err(McpScopeCeilingUseCaseError::Invalid);
        }
        self.validate_principal_scope_ceiling(&scopes)?;
        self.scope_ceiling_repository
            .replace_principal_scope_ceiling(&actor, &scopes, expected_revision, self.clock.now())
            .await
            .map_err(map_scope_ceiling_repository_error)
    }

    pub async fn principal_scope_ceiling(
        &self,
        actor: Actor,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingUseCaseError> {
        let stored = self
            .scope_ceiling_repository
            .principal_scope_ceiling(&actor)
            .await
            .map_err(map_scope_ceiling_repository_error)?;
        Ok(stored.unwrap_or_else(|| McpScopeCeilingSetting {
            scopes: self.authorization_server.supported_scopes().to_vec(),
            revision: 0,
        }))
    }
    pub async fn client_authorizations(
        &self,
        actor: Actor,
    ) -> Result<Vec<McpClientAuthorization>, McpScopeCeilingUseCaseError> {
        self.scope_ceiling_repository
            .client_authorizations(&actor, self.clock.now())
            .await
            .map_err(map_scope_ceiling_repository_error)
    }

    pub async fn replace_client_scope_ceiling(
        &self,
        actor: Actor,
        client_id: String,
        scopes: Vec<String>,
        expected_revision: i64,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingUseCaseError> {
        if expected_revision < 0 {
            return Err(McpScopeCeilingUseCaseError::Invalid);
        }
        self.validate_client_scope_ceiling(&scopes)?;
        self.scope_ceiling_repository
            .replace_client_scope_ceiling(
                &actor,
                &client_id,
                &scopes,
                expected_revision,
                self.clock.now(),
            )
            .await
            .map_err(map_scope_ceiling_repository_error)
    }

    fn validate_principal_scope_ceiling(
        &self,
        scopes: &[String],
    ) -> Result<(), McpScopeCeilingUseCaseError> {
        self.authorization_server
            .scope_ceilings(
                scopes.to_vec(),
                self.authorization_server.supported_scopes().to_vec(),
            )
            .map(|_| ())
            .map_err(|_| McpScopeCeilingUseCaseError::Invalid)
    }

    fn validate_client_scope_ceiling(
        &self,
        scopes: &[String],
    ) -> Result<(), McpScopeCeilingUseCaseError> {
        self.authorization_server
            .scope_ceilings(
                self.authorization_server.supported_scopes().to_vec(),
                scopes.to_vec(),
            )
            .map(|_| ())
            .map_err(|_| McpScopeCeilingUseCaseError::Invalid)
    }

    pub async fn exchange_authorization_code(
        &self,
        code: String,
        client_id: String,
        redirect_uri: Option<String>,
        resource_uri: String,
        verifier: String,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        self.authorization_server
            .exchange_authorization_code(code, client_id, redirect_uri, resource_uri, verifier)
            .await
    }

    pub async fn refresh_access_token(
        &self,
        refresh_token: String,
        client_id: String,
        resource_uri: String,
        scopes: Option<Vec<String>>,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        self.authorization_server
            .refresh_access_token(refresh_token, client_id, resource_uri, scopes)
            .await
    }

    pub async fn authenticate(
        &self,
        token: &str,
        resource_uri: &str,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError> {
        let Some(authenticated) = self
            .authorization_server
            .authenticate(token, resource_uri)
            .await?
        else {
            return Ok(None);
        };
        let actor = Actor::try_new(
            authenticated.principal.issuer().into(),
            authenticated.principal.subject().into(),
        )
        .map_err(|_| McpOAuthUseCaseError::Unavailable)?;
        Ok(Some(McpAuthenticatedActor {
            actor,
            scopes: authenticated.scopes,
        }))
    }

    pub async fn revoke(&self, actor: &Actor, client_id: &str) -> Result<(), McpOAuthUseCaseError> {
        self.authorization_server
            .revoke(&principal(actor), client_id)
            .await
    }

    pub async fn revoke_token(
        &self,
        token: &str,
        client_id: &str,
    ) -> Result<(), McpOAuthUseCaseError> {
        self.authorization_server
            .revoke_token(token, client_id)
            .await
    }
}

fn principal(actor: &Actor) -> Principal {
    Principal::new(actor.issuer().into(), actor.subject().into())
}

fn map_scope_ceiling_repository_error(
    error: McpScopeCeilingRepositoryError,
) -> McpScopeCeilingUseCaseError {
    match error {
        McpScopeCeilingRepositoryError::Invalid => McpScopeCeilingUseCaseError::Invalid,
        McpScopeCeilingRepositoryError::Conflict => McpScopeCeilingUseCaseError::Conflict,
        McpScopeCeilingRepositoryError::CorruptData => McpScopeCeilingUseCaseError::CorruptData,
        McpScopeCeilingRepositoryError::ClientNotFound => {
            McpScopeCeilingUseCaseError::ClientNotFound
        }
        McpScopeCeilingRepositoryError::Unavailable => McpScopeCeilingUseCaseError::Unavailable,
    }
}

#[async_trait]
impl McpOAuthUseCases for McpOAuthApplication {
    fn resource_policy(&self) -> McpResourcePolicy {
        McpOAuthApplication::resource_policy(self).clone()
    }

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

    async fn principal_scope_ceiling(
        &self,
        actor: Actor,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingUseCaseError> {
        McpOAuthApplication::principal_scope_ceiling(self, actor).await
    }

    async fn client_authorizations(
        &self,
        actor: Actor,
    ) -> Result<Vec<McpClientAuthorization>, McpScopeCeilingUseCaseError> {
        McpOAuthApplication::client_authorizations(self, actor).await
    }

    async fn replace_principal_scope_ceiling(
        &self,
        actor: Actor,
        scopes: Vec<String>,
        expected_revision: i64,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingUseCaseError> {
        McpOAuthApplication::replace_principal_scope_ceiling(self, actor, scopes, expected_revision)
            .await
    }

    async fn replace_client_scope_ceiling(
        &self,
        actor: Actor,
        client_id: String,
        scopes: Vec<String>,
        expected_revision: i64,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingUseCaseError> {
        McpOAuthApplication::replace_client_scope_ceiling(
            self,
            actor,
            client_id,
            scopes,
            expected_revision,
        )
        .await
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
