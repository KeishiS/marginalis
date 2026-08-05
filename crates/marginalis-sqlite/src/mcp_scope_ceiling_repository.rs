//! 利用者とMCPクライアントのscope上限を扱うSQLite実装。

use async_trait::async_trait;
use marginalis_application::{
    McpClientAuthorization, McpClientRegistrationMethod, McpScopeCeilingRepository,
    McpScopeCeilingRepositoryError, McpScopeCeilingSetting, McpStoredScopeCeilings,
};
use marginalis_domain::{Actor, UnixMillis};
use sqlx::{Row, Sqlite, Transaction};

use crate::SqliteDatabase;

#[async_trait]
impl McpScopeCeilingRepository for SqliteDatabase {
    async fn client_authorizations(
        &self,
        actor: &Actor,
        now: UnixMillis,
    ) -> Result<Vec<McpClientAuthorization>, McpScopeCeilingRepositoryError> {
        let rows = sqlx::query(
            "SELECT clients.client_id, clients.display_name, clients.registration_method,
                    authorizations.granted_scopes, authorizations.authorized_at_ms,
                    authorizations.last_used_at_ms,
                    ceilings.scopes AS ceiling_scopes,
                    ceilings.revision AS ceiling_revision,
                    (authorizations.revoked_at_ms IS NULL AND (
                        EXISTS(SELECT 1 FROM mcp_authorization_codes AS codes
                               WHERE codes.issuer = authorizations.issuer
                                 AND codes.subject = authorizations.subject
                                 AND codes.client_id = authorizations.client_id
                                 AND codes.consumed_at_ms IS NULL AND codes.expires_at_ms > ?)
                        OR EXISTS(SELECT 1 FROM mcp_access_tokens AS access_tokens
                                  WHERE access_tokens.issuer = authorizations.issuer
                                    AND access_tokens.subject = authorizations.subject
                                    AND access_tokens.client_id = authorizations.client_id
                                    AND access_tokens.revoked_at_ms IS NULL
                                    AND access_tokens.expires_at_ms > ?)
                        OR EXISTS(SELECT 1 FROM mcp_refresh_tokens AS refresh_tokens
                                  WHERE refresh_tokens.issuer = authorizations.issuer
                                    AND refresh_tokens.subject = authorizations.subject
                                    AND refresh_tokens.client_id = authorizations.client_id
                                    AND refresh_tokens.rotated_at_ms IS NULL
                                    AND refresh_tokens.revoked_at_ms IS NULL
                                    AND refresh_tokens.expires_at_ms > ?)
                    )) AS active
             FROM mcp_client_authorizations AS authorizations
             JOIN mcp_clients AS clients ON clients.client_id = authorizations.client_id
             LEFT JOIN mcp_client_scope_ceilings AS ceilings
                    ON ceilings.issuer = authorizations.issuer
                   AND ceilings.subject = authorizations.subject
                   AND ceilings.client_id = authorizations.client_id
             WHERE authorizations.issuer = ? AND authorizations.subject = ?
             ORDER BY authorizations.authorized_at_ms DESC, clients.client_id ASC",
        )
        .bind(now.get())
        .bind(now.get())
        .bind(now.get())
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_all(&self.pool)
        .await
        .map_err(map_database_error)?;
        rows.into_iter().map(decode_authorization).collect()
    }

    async fn principal_scope_ceiling(
        &self,
        actor: &Actor,
    ) -> Result<Option<McpScopeCeilingSetting>, McpScopeCeilingRepositoryError> {
        sqlx::query(
            "SELECT scopes, revision FROM mcp_principal_scope_ceilings
             WHERE issuer = ? AND subject = ?",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_database_error)?
        .map(decode_setting)
        .transpose()
    }

    async fn scope_ceilings(
        &self,
        actor: &Actor,
        client_id: &str,
    ) -> Result<McpStoredScopeCeilings, McpScopeCeilingRepositoryError> {
        let principal = self.principal_scope_ceiling(actor).await?;
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
        invalidate_excess_grants(&mut transaction, actor, None, scopes, now).await?;
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
        let granted_scopes = sqlx::query_scalar::<_, String>(
            "SELECT granted_scopes FROM mcp_client_authorizations
             WHERE issuer = ? AND subject = ? AND client_id = ?",
        )
        .bind(actor.issuer())
        .bind(actor.subject())
        .bind(client_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_database_error)?
        .ok_or(McpScopeCeilingRepositoryError::ClientNotFound)?;
        let granted_scopes = granted_scopes.split_ascii_whitespace().collect::<Vec<_>>();
        if scopes
            .iter()
            .any(|scope| !granted_scopes.contains(&scope.as_str()))
        {
            return Err(McpScopeCeilingRepositoryError::Invalid);
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
        invalidate_excess_grants(&mut transaction, actor, Some(client_id), scopes, now).await?;
        transaction.commit().await.map_err(map_database_error)?;
        Ok(McpScopeCeilingSetting {
            scopes: scopes.to_vec(),
            revision,
        })
    }
}

fn decode_authorization(
    row: sqlx::sqlite::SqliteRow,
) -> Result<McpClientAuthorization, McpScopeCeilingRepositoryError> {
    let registration_method = match row
        .try_get::<String, _>("registration_method")
        .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?
        .as_str()
    {
        "dynamic" => McpClientRegistrationMethod::Dynamic,
        "metadata_document" => McpClientRegistrationMethod::MetadataDocument,
        _ => return Err(McpScopeCeilingRepositoryError::CorruptData),
    };
    let ceiling_scopes = row
        .try_get::<Option<String>, _>("ceiling_scopes")
        .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?;
    let ceiling_revision = row
        .try_get::<Option<i64>, _>("ceiling_revision")
        .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?;
    let scope_ceiling = match (ceiling_scopes, ceiling_revision) {
        (None, None) => None,
        (Some(scopes), Some(revision)) if revision >= 1 => Some(McpScopeCeilingSetting {
            scopes: split_scope_value(&scopes),
            revision,
        }),
        _ => return Err(McpScopeCeilingRepositoryError::CorruptData),
    };
    Ok(McpClientAuthorization {
        client_id: row
            .try_get("client_id")
            .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?,
        display_name: row
            .try_get("display_name")
            .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?,
        registration_method,
        granted_scopes: split_scopes(&row, "granted_scopes")?,
        scope_ceiling,
        authorized_at: UnixMillis::new(
            row.try_get("authorized_at_ms")
                .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?,
        ),
        last_used_at: row
            .try_get::<Option<i64>, _>("last_used_at_ms")
            .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?
            .map(UnixMillis::new),
        active: row
            .try_get("active")
            .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?,
    })
}

fn split_scopes(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Vec<String>, McpScopeCeilingRepositoryError> {
    Ok(row
        .try_get::<String, _>(column)
        .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect())
}

fn split_scope_value(scopes: &str) -> Vec<String> {
    scopes.split_ascii_whitespace().map(str::to_owned).collect()
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

async fn invalidate_excess_grants(
    transaction: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    client_id: Option<&str>,
    allowed_scopes: &[String],
    now: UnixMillis,
) -> Result<(), McpScopeCeilingRepositoryError> {
    let code_query = if client_id.is_some() {
        "SELECT code_hash, scopes FROM mcp_authorization_codes
         WHERE issuer = ? AND subject = ? AND client_id = ? AND consumed_at_ms IS NULL"
    } else {
        "SELECT code_hash, scopes FROM mcp_authorization_codes
         WHERE issuer = ? AND subject = ? AND consumed_at_ms IS NULL"
    };
    let mut code_query = sqlx::query(code_query)
        .bind(actor.issuer())
        .bind(actor.subject());
    if let Some(client_id) = client_id {
        code_query = code_query.bind(client_id);
    }
    let codes = code_query
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_database_error)?;
    for row in codes {
        let scopes = row
            .try_get::<String, _>("scopes")
            .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?;
        if scopes_fit(&scopes, allowed_scopes) {
            continue;
        }
        let code_hash = row
            .try_get::<Vec<u8>, _>("code_hash")
            .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?;
        sqlx::query("DELETE FROM mcp_authorization_codes WHERE code_hash = ?")
            .bind(code_hash)
            .execute(&mut **transaction)
            .await
            .map_err(map_database_error)?;
    }

    let family_query = if client_id.is_some() {
        "SELECT token_family_id, scopes FROM mcp_access_tokens
         WHERE issuer = ? AND subject = ? AND client_id = ? AND revoked_at_ms IS NULL
         UNION
         SELECT token_family_id, scopes FROM mcp_refresh_tokens
         WHERE issuer = ? AND subject = ? AND client_id = ? AND revoked_at_ms IS NULL"
    } else {
        "SELECT token_family_id, scopes FROM mcp_access_tokens
         WHERE issuer = ? AND subject = ? AND revoked_at_ms IS NULL
         UNION
         SELECT token_family_id, scopes FROM mcp_refresh_tokens
         WHERE issuer = ? AND subject = ? AND revoked_at_ms IS NULL"
    };
    let mut family_query = sqlx::query(family_query)
        .bind(actor.issuer())
        .bind(actor.subject());
    if let Some(client_id) = client_id {
        family_query = family_query.bind(client_id);
    }
    family_query = family_query.bind(actor.issuer()).bind(actor.subject());
    if let Some(client_id) = client_id {
        family_query = family_query.bind(client_id);
    }
    let families = family_query
        .fetch_all(&mut **transaction)
        .await
        .map_err(map_database_error)?;
    for row in families {
        let scopes = row
            .try_get::<String, _>("scopes")
            .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?;
        if scopes_fit(&scopes, allowed_scopes) {
            continue;
        }
        let family = row
            .try_get::<Vec<u8>, _>("token_family_id")
            .map_err(|_| McpScopeCeilingRepositoryError::CorruptData)?;
        for table in ["mcp_access_tokens", "mcp_refresh_tokens"] {
            sqlx::query(&format!(
                "UPDATE {table} SET revoked_at_ms = ?
                 WHERE token_family_id = ? AND revoked_at_ms IS NULL"
            ))
            .bind(now.get())
            .bind(&family)
            .execute(&mut **transaction)
            .await
            .map_err(map_database_error)?;
        }
    }
    Ok(())
}

fn scopes_fit(encoded: &str, allowed_scopes: &[String]) -> bool {
    encoded
        .split_ascii_whitespace()
        .all(|scope| allowed_scopes.iter().any(|allowed| allowed == scope))
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
