//! applicationのMCP OAuth repository portに対するSQLite実装。

use async_trait::async_trait;
use marginalis_application::{
    McpAuthenticatedPrincipal, McpAuthorizationCodeExchange, McpAuthorizationGrant, McpOAuthClient,
    McpOAuthRepository, McpOAuthRepositoryError, McpRefreshTokenRotation,
    McpRefreshTokenRotationOutcome, McpRegisteredOAuthClient, McpTimestamp,
};

use crate::SqliteDatabase;

#[async_trait]
impl McpOAuthRepository for SqliteDatabase {
    async fn register_client_bounded(
        &self,
        client: &McpOAuthClient,
        now: McpTimestamp,
        maximum_clients: i64,
    ) -> Result<bool, McpOAuthRepositoryError> {
        self.register_mcp_client_bounded(client, now, maximum_clients)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn client(
        &self,
        client_id: &str,
    ) -> Result<Option<McpRegisteredOAuthClient>, McpOAuthRepositoryError> {
        self.registered_mcp_client(client_id)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn issue_authorization_code(
        &self,
        code: &str,
        client: &McpRegisteredOAuthClient,
        grant: &McpAuthorizationGrant,
        code_challenge: &str,
        expires_at: McpTimestamp,
        now: McpTimestamp,
    ) -> Result<(), McpOAuthRepositoryError> {
        self.issue_mcp_authorization_code(code, client, grant, code_challenge, expires_at, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn exchange_authorization_code(
        &self,
        exchange: McpAuthorizationCodeExchange,
        now: McpTimestamp,
    ) -> Result<Option<McpAuthorizationGrant>, McpOAuthRepositoryError> {
        self.exchange_mcp_authorization_code(exchange, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn rotate_refresh_token(
        &self,
        rotation: McpRefreshTokenRotation,
        now: McpTimestamp,
    ) -> Result<McpRefreshTokenRotationOutcome, McpOAuthRepositoryError> {
        self.rotate_mcp_refresh_token(rotation, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn authenticate_access_token(
        &self,
        token: &str,
        resource_uri: &str,
        now: McpTimestamp,
    ) -> Result<Option<McpAuthenticatedPrincipal>, McpOAuthRepositoryError> {
        self.authenticate_mcp_access_token(token, resource_uri, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn revoke_client_tokens(
        &self,
        issuer: &str,
        subject: &str,
        client_id: &str,
        now: McpTimestamp,
    ) -> Result<(), McpOAuthRepositoryError> {
        self.revoke_mcp_client_tokens(issuer, subject, client_id, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }

    async fn revoke_token(
        &self,
        token: &str,
        client_id: &str,
        now: McpTimestamp,
    ) -> Result<(), McpOAuthRepositoryError> {
        self.revoke_mcp_token(token, client_id, now)
            .await
            .map_err(|_| McpOAuthRepositoryError)
    }
}
