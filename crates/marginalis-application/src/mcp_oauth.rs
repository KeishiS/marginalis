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
    McpClientMetadataResolver, McpOAuthClient, McpOAuthRepository, McpOAuthUseCaseError,
    McpOAuthUseCases, McpResourcePolicy, McpTokenPair, McpValidatedAuthorizationRequest, Random,
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
}

impl McpOAuthApplication {
    pub fn new(
        repository: Arc<dyn McpOAuthRepository>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
        resource_policy: McpResourcePolicy,
    ) -> Self {
        Self {
            authorization_server: AuthorizationServer::new(
                repository,
                Arc::new(ClockAdapter(clock)),
                Arc::new(RandomAdapter(random)),
                resource_policy,
                MCP_AUTHORIZATION_CONFIG,
            ),
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
        // v0.28までの動作を保つ暫定値。#268で保存した利用者上限とclient上限へ置き換える。
        let ceilings = self
            .authorization_server
            .scope_ceilings(request.scopes.clone(), request.scopes.clone())?;
        self.authorization_server
            .authorize(principal(&actor), request, &ceilings)
            .await
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
