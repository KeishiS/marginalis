//! 利用者とMCPクライアントのscope上限を扱うSQLite実装。

use async_trait::async_trait;
use marginalis_application::{
    McpScopeCeilingRepository, McpScopeCeilingRepositoryError, McpScopeCeilingSetting,
    McpStoredScopeCeilings,
};
use marginalis_domain::{Actor, UnixMillis};
use sqlx::{Row, Sqlite, Transaction};

use crate::SqliteDatabase;

#[async_trait]
impl McpScopeCeilingRepository for SqliteDatabase {
    async fn scope_ceilings(
        &self,
        actor: &Actor,
        client_id: &str,
    ) -> Result<McpStoredScopeCeilings, McpScopeCeilingRepositoryError> {
        let principal = sqlx::query(
            "SELECT scopes, revision FROM mcp_principal_scope_ceilings
             WHERE issuer = ? AND subject = ?",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_database_error)?
        .map(decode_setting)
        .transpose()?;
        let client = sqlx::query(
            "SELECT scopes, revision FROM mcp_client_scope_ceilings
             WHERE issuer = ? AND subject = ? AND client_id = ?",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(client_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_database_error)?
        .map(decode_setting)
        .transpose()?;
        Ok(McpStoredScopeCeilings { principal, client })
    }

    async fn replace_principal_scope_ceiling(
        &self,
        actor: &Actor,
        scopes: &[String],
        expected_revision: i64,
        now: UnixMillis,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(map_database_error)?;
        let revision =
            replace_principal_setting(&mut transaction, actor, scopes, expected_revision, now)
                .await?;
        invalidate_principal_grants(&mut transaction, actor, None, now).await?;
        transaction.commit().await.map_err(map_database_error)?;
        Ok(McpScopeCeilingSetting {
            scopes: scopes.to_vec(),
            revision,
        })
    }

    async fn replace_client_scope_ceiling(
        &self,
        actor: &Actor,
        client_id: &str,
        scopes: &[String],
        expected_revision: i64,
        now: UnixMillis,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(map_database_error)?;
        let client_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM mcp_clients WHERE client_id = ?)",
        )
        .bind(client_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_database_error)?;
        if !client_exists {
            return Err(McpScopeCeilingRepositoryError::ClientNotFound);
        }
        let revision = replace_client_setting(
            &mut transaction,
            actor,
            client_id,
            scopes,
            expected_revision,
            now,
        )
        .await?;
        invalidate_principal_grants(&mut transaction, actor, Some(client_id), now).await?;
        transaction.commit().await.map_err(map_database_error)?;
        Ok(McpScopeCeilingSetting {
            scopes: scopes.to_vec(),
            revision,
        })
    }
}

async fn replace_principal_setting(
    transaction: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    scopes: &[String],
    expected_revision: i64,
    now: UnixMillis,
) -> Result<i64, McpScopeCeilingRepositoryError> {
    let encoded = scopes.join(" ");
    if expected_revision == 0 {
        let result = sqlx::query(
            "INSERT INTO mcp_principal_scope_ceilings
                 (issuer, subject, scopes, revision, updated_at_ms)
             VALUES (?, ?, ?, 1, ?)
             ON CONFLICT (issuer, subject) DO NOTHING",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(encoded)
        .bind(now.get())
        .execute(&mut **transaction)
        .await
        .map_err(map_database_error)?;
        if result.rows_affected() != 1 {
            return Err(McpScopeCeilingRepositoryError::Conflict);
        }
        return Ok(1);
    }
    sqlx::query_scalar::<_, i64>(
        "UPDATE mcp_principal_scope_ceilings
         SET scopes = ?, revision = revision + 1, updated_at_ms = ?
         WHERE issuer = ? AND subject = ? AND revision = ?
         RETURNING revision",
    )
    .bind(encoded)
    .bind(now.get())
    .bind(actor.issuer())
    .bind(actor.subject())
    .bind(expected_revision)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_database_error)?
    .ok_or(McpScopeCeilingRepositoryError::Conflict)
}

async fn replace_client_setting(
    transaction: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    client_id: &str,
    scopes: &[String],
    expected_revision: i64,
    now: UnixMillis,
) -> Result<i64, McpScopeCeilingRepositoryError> {
    let encoded = scopes.join(" ");
    if expected_revision == 0 {
        let result = sqlx::query(
            "INSERT INTO mcp_client_scope_ceilings
                 (issuer, subject, client_id, scopes, revision, updated_at_ms)
             VALUES (?, ?, ?, ?, 1, ?)
             ON CONFLICT (issuer, subject, client_id) DO NOTHING",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(client_id)
        .bind(encoded)
        .bind(now.get())
        .execute(&mut **transaction)
        .await
        .map_err(map_database_error)?;
        if result.rows_affected() != 1 {
            return Err(McpScopeCeilingRepositoryError::Conflict);
        }
        return Ok(1);
    }
    sqlx::query_scalar::<_, i64>(
        "UPDATE mcp_client_scope_ceilings
         SET scopes = ?, revision = revision + 1, updated_at_ms = ?
         WHERE issuer = ? AND subject = ? AND client_id = ? AND revision = ?
         RETURNING revision",
    )
    .bind(encoded)
    .bind(now.get())
    .bind(actor.issuer())
    .bind(actor.subject())
    .bind(client_id)
    .bind(expected_revision)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_database_error)?
    .ok_or(McpScopeCeilingRepositoryError::Conflict)
}

async fn invalidate_principal_grants(
    transaction: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    client_id: Option<&str>,
    now: UnixMillis,
) -> Result<(), McpScopeCeilingRepositoryError> {
    if let Some(client_id) = client_id {
        sqlx::query(
            "DELETE FROM mcp_authorization_codes
             WHERE issuer = ? AND subject = ? AND client_id = ?",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(client_id)
        .execute(&mut **transaction)
        .await
        .map_err(map_database_error)?;
    } else {
        sqlx::query("DELETE FROM mcp_authorization_codes WHERE issuer = ? AND subject = ?")
            .bind(actor.issuer())
            .bind(actor.subject())
            .execute(&mut **transaction)
            .await
            .map_err(map_database_error)?;
    }
    for table in ["mcp_access_tokens", "mcp_refresh_tokens"] {
        let query = if client_id.is_some() {
            format!(
                "UPDATE {table} SET revoked_at_ms = ?
                 WHERE issuer = ? AND subject = ? AND client_id = ? AND revoked_at_ms IS NULL"
            )
        } else {
            format!(
                "UPDATE {table} SET revoked_at_ms = ?
                 WHERE issuer = ? AND subject = ? AND revoked_at_ms IS NULL"
            )
        };
        let mut query = sqlx::query(&query)
            .bind(now.get())
            .bind(actor.issuer())
            .bind(actor.subject());
        if let Some(client_id) = client_id {
            query = query.bind(client_id);
        }
        query
            .execute(&mut **transaction)
            .await
            .map_err(map_database_error)?;
    }
    Ok(())
}

fn decode_setting(
    row: sqlx::sqlite::SqliteRow,
) -> Result<McpScopeCeilingSetting, McpScopeCeilingRepositoryError> {
    let value = row
        .try_get::<String, _>("scopes")
        .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?;
    let revision = row
        .try_get::<i64, _>("revision")
        .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?;
    if revision <= 0 {
        return Err(McpScopeCeilingRepositoryError::CorruptData);
    }
    Ok(McpScopeCeilingSetting {
        scopes: value.split_ascii_whitespace().map(str::to_owned).collect(),
        revision,
    })
}

fn map_database_error(_: sqlx::Error) -> McpScopeCeilingRepositoryError {
    McpScopeCeilingRepositoryError::Unavailable
}
