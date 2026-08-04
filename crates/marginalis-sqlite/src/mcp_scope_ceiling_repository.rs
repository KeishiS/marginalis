//! 利用者とMCPクライアントのscope上限を読み取るSQLite実装。

use async_trait::async_trait;
use marginalis_application::{
    McpScopeCeilingRepository, McpScopeCeilingRepositoryError, McpStoredScopeCeilings,
};
use marginalis_domain::Actor;

use crate::SqliteDatabase;

#[async_trait]
impl McpScopeCeilingRepository for SqliteDatabase {
    async fn scope_ceilings(
        &self,
        actor: &Actor,
        client_id: &str,
    ) -> Result<McpStoredScopeCeilings, McpScopeCeilingRepositoryError> {
        let principal = sqlx::query_scalar::<_, String>(
            "SELECT scopes FROM mcp_principal_scope_ceilings WHERE issuer = ? AND subject = ?",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| McpScopeCeilingRepositoryError)?
        .map(|value| decode_scopes(&value));
        let client = sqlx::query_scalar::<_, String>(
            "SELECT scopes FROM mcp_client_scope_ceilings
             WHERE issuer = ? AND subject = ? AND client_id = ?",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(client_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| McpScopeCeilingRepositoryError)?
        .map(|value| decode_scopes(&value));
        Ok(McpStoredScopeCeilings { principal, client })
    }
}

fn decode_scopes(value: &str) -> Vec<String> {
    value.split_ascii_whitespace().map(str::to_owned).collect()
}
