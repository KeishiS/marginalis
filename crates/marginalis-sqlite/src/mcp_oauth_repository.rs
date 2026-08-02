//! applicationのMCP OAuth repository portに対するSQLite実装。

use async_trait::async_trait;
use marginalis_application::{
    McpAuthorizationCodeExchange, McpOAuthRepository, McpOAuthRepositoryError,
    McpRefreshTokenRotation, McpRefreshTokenRotationOutcome,
};
use marginalis_domain::{McpAuthenticatedActor, McpAuthorizationGrant, McpOAuthClient, UnixMillis};

use crate::SqliteDatabase;

#[async_trait]
impl McpOAuthRepository for SqliteDatabase {
    async fn register_client_bounded(
        &self,
        client: &McpOAuthClient,
        now: UnixMillis,
        maximum_clients: i64,
    ) -> Result<bool, McpOAuthRepositoryError> {
        self.register_mcp_client_bounded(client, now, maximum_clients)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn client(
        &self,
        client_id: &str,
    ) -> Result<Option<McpOAuthClient>, McpOAuthRepositoryError> {
        self.mcp_client(client_id)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn issue_authorization_code(
        &self,
        code: &str,
        grant: &McpAuthorizationGrant,
        code_challenge: &str,
        expires_at: UnixMillis,
    ) -> Result<(), McpOAuthRepositoryError> {
        self.issue_mcp_authorization_code(code, grant, code_challenge, expires_at)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn exchange_authorization_code(
        &self,
        exchange: McpAuthorizationCodeExchange,
        now: UnixMillis,
    ) -> Result<Option<McpAuthorizationGrant>, McpOAuthRepositoryError> {
        self.exchange_mcp_authorization_code(exchange, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn rotate_refresh_token(
        &self,
        rotation: McpRefreshTokenRotation,
        now: UnixMillis,
    ) -> Result<McpRefreshTokenRotationOutcome, McpOAuthRepositoryError> {
        self.rotate_mcp_refresh_token(rotation, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn authenticate_access_token(
        &self,
        token: &str,
        resource_uri: &str,
        now: UnixMillis,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthRepositoryError> {
        self.authenticate_mcp_access_token(token, resource_uri, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn revoke_client_tokens(
        &self,
        issuer: &str,
        subject: &str,
        client_id: &str,
        now: UnixMillis,
    ) -> Result<(), McpOAuthRepositoryError> {
        self.revoke_mcp_client_tokens(issuer, subject, client_id, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn revoke_token(
        &self,
        token: &str,
        client_id: &str,
        now: UnixMillis,
    ) -> Result<(), McpOAuthRepositoryError> {
        self.revoke_mcp_token(token, client_id, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }
}
