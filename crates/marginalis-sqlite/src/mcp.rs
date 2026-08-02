//! MCP OAuth client、code、access/refresh tokenの永続化。

use marginalis_application::{
    McpAuthorizationCodeExchange, McpRefreshTokenRotation, McpRefreshTokenRotationOutcome,
};
use marginalis_domain::{
    Actor, McpAuthenticatedActor, McpAuthorizationGrant, McpOAuthClient, UnixMillis,
};
use sqlx::Row;

use crate::{SqliteDatabase, SqliteStoreError, database_error, token::hash_token};

impl SqliteDatabase {
    pub async fn issue_mcp_authorization_code(
        &self,
        code: &str,
        grant: &McpAuthorizationGrant,
        code_challenge: &str,
        expires_at: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        sqlx::query("INSERT INTO mcp_authorization_codes (code_hash, client_id, redirect_uri, resource_uri, issuer, subject, scopes, code_challenge, expires_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(code)).bind(&grant.client_id).bind(&grant.redirect_uri).bind(&grant.resource_uri)
            .bind(grant.actor.issuer()).bind(grant.actor.subject())
            .bind(grant.scopes.join(" ")).bind(code_challenge).bind(expires_at.get())
            .execute(&self.pool).await.map_err(database_error)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn upsert_mcp_client(
        &self,
        client: &McpOAuthClient,
        registered_at: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        let redirect_uris = serde_json::to_string(&client.redirect_uris)
            .map_err(|_| SqliteStoreError::CorruptData)?;
        sqlx::query("INSERT INTO mcp_clients (client_id, display_name, redirect_uris_json, registered_at_ms) VALUES (?, ?, ?, ?) ON CONFLICT(client_id) DO UPDATE SET display_name = excluded.display_name, redirect_uris_json = excluded.redirect_uris_json")
            .bind(&client.client_id).bind(&client.display_name).bind(redirect_uris).bind(registered_at.get()).execute(&self.pool).await.map_err(database_error)?;
        Ok(())
    }

    /// configured persistence boundに空きがある場合だけclientを原子的に登録する。
    pub async fn register_mcp_client_bounded(
        &self,
        client: &McpOAuthClient,
        now: UnixMillis,
        maximum_clients: i64,
    ) -> Result<bool, SqliteStoreError> {
        let redirect_uris = serde_json::to_string(&client.redirect_uris)
            .map_err(|_| SqliteStoreError::CorruptData)?;
        let result = sqlx::query(
            "INSERT INTO mcp_clients
                 (client_id, display_name, redirect_uris_json, registered_at_ms)
             SELECT ?, ?, ?, ?
             WHERE (SELECT COUNT(*) FROM mcp_clients) < ?",
        )
        .bind(&client.client_id)
        .bind(&client.display_name)
        .bind(redirect_uris)
        .bind(now.get())
        .bind(maximum_clients)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn mcp_client(
        &self,
        client_id: &str,
    ) -> Result<Option<McpOAuthClient>, SqliteStoreError> {
        let row = sqlx::query("SELECT client_id, display_name, redirect_uris_json FROM mcp_clients WHERE client_id = ?")
            .bind(client_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        row.map(|row| {
            Ok(McpOAuthClient {
                client_id: row.try_get("client_id").map_err(database_error)?,
                display_name: row.try_get("display_name").map_err(database_error)?,
                redirect_uris: serde_json::from_str(
                    &row.try_get::<String, _>("redirect_uris_json")
                        .map_err(database_error)?,
                )
                .map_err(|_| SqliteStoreError::CorruptData)?,
            })
        })
        .transpose()
    }

    /// 認可codeを一度だけ消費してtoken pairを発行し、再利用時は発行済みfamilyを失効する。
    pub async fn exchange_mcp_authorization_code(
        &self,
        exchange: McpAuthorizationCodeExchange,
        now: UnixMillis,
    ) -> Result<Option<McpAuthorizationGrant>, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let code_hash = hash_token(&exchange.code);
        let row = sqlx::query(
            "UPDATE mcp_authorization_codes SET consumed_at_ms = ?
             WHERE code_hash = ? AND client_id = ?
               AND redirect_uri = ?
               AND resource_uri = ? AND code_challenge = ?
               AND consumed_at_ms IS NULL AND expires_at_ms > ?
             RETURNING redirect_uri, issuer, subject, scopes",
        )
        .bind(now.get())
        .bind(&code_hash)
        .bind(&exchange.client_id)
        .bind(exchange.redirect_uri.as_deref())
        .bind(&exchange.resource_uri)
        .bind(&exchange.code_challenge)
        .bind(now.get())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database_error)?;
        let Some(row) = row else {
            let replayed_family = sqlx::query_scalar::<_, Vec<u8>>(
                "SELECT token_family_id FROM mcp_authorization_codes
                 WHERE code_hash = ? AND client_id = ?
                   AND redirect_uri = ?
                   AND resource_uri = ? AND code_challenge = ?
                   AND consumed_at_ms IS NOT NULL
                   AND token_family_id IS NOT NULL",
            )
            .bind(&code_hash)
            .bind(&exchange.client_id)
            .bind(exchange.redirect_uri.as_deref())
            .bind(&exchange.resource_uri)
            .bind(&exchange.code_challenge)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(database_error)?;
            if let Some(token_family_id) = replayed_family {
                for table in ["mcp_access_tokens", "mcp_refresh_tokens"] {
                    let query = format!(
                        "UPDATE {table} SET revoked_at_ms = ?
                         WHERE token_family_id = ? AND revoked_at_ms IS NULL"
                    );
                    sqlx::query(&query)
                        .bind(now.get())
                        .bind(&token_family_id)
                        .execute(&mut *transaction)
                        .await
                        .map_err(database_error)?;
                }
            }
            transaction.commit().await.map_err(database_error)?;
            return Ok(None);
        };
        let grant = McpAuthorizationGrant {
            actor: actor_from_row(&row)?,
            client_id: exchange.client_id,
            redirect_uri: row.try_get("redirect_uri").map_err(database_error)?,
            resource_uri: exchange.resource_uri,
            scopes: row
                .try_get::<String, _>("scopes")
                .map_err(database_error)?
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        };
        let scopes = grant.scopes.join(" ");
        let token_family_id = hash_token(&exchange.refresh_token);
        sqlx::query("INSERT INTO mcp_access_tokens (token_hash, client_id, resource_uri, issuer, subject, scopes, expires_at_ms, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(&exchange.access_token)).bind(&grant.client_id).bind(&grant.resource_uri).bind(grant.actor.issuer()).bind(grant.actor.subject()).bind(&scopes).bind(exchange.access_expires_at.get()).bind(&token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        sqlx::query("INSERT INTO mcp_refresh_tokens (token_hash, client_id, resource_uri, issuer, subject, scopes, expires_at_ms, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(&exchange.refresh_token)).bind(&grant.client_id).bind(&grant.resource_uri).bind(grant.actor.issuer()).bind(grant.actor.subject()).bind(scopes).bind(exchange.refresh_expires_at.get()).bind(&token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        sqlx::query("UPDATE mcp_authorization_codes SET token_family_id = ? WHERE code_hash = ?")
            .bind(token_family_id)
            .bind(code_hash)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(Some(grant))
    }

    pub async fn authenticate_mcp_access_token(
        &self,
        token: &str,
        resource_uri: &str,
        now: UnixMillis,
    ) -> Result<Option<McpAuthenticatedActor>, SqliteStoreError> {
        let row = sqlx::query("SELECT issuer, subject, scopes FROM mcp_access_tokens WHERE token_hash = ? AND resource_uri = ? AND revoked_at_ms IS NULL AND expires_at_ms > ?")
            .bind(hash_token(token)).bind(resource_uri).bind(now.get()).fetch_optional(&self.pool).await.map_err(database_error)?;
        row.map(|r| {
            Ok(McpAuthenticatedActor {
                actor: actor_from_row(&r)?,
                scopes: r
                    .try_get::<String, _>("scopes")
                    .map_err(database_error)?
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect(),
            })
        })
        .transpose()
    }

    /// refresh tokenを一度だけ消費し、同じKanidm主体に新しいtoken pairを発行する。
    pub async fn rotate_mcp_refresh_token(
        &self,
        rotation: McpRefreshTokenRotation,
        now: UnixMillis,
    ) -> Result<McpRefreshTokenRotationOutcome, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let row = sqlx::query(
            "UPDATE mcp_refresh_tokens SET rotated_at_ms = ?
             WHERE token_hash = ? AND client_id = ? AND resource_uri = ?
               AND rotated_at_ms IS NULL AND revoked_at_ms IS NULL AND expires_at_ms > ?
             RETURNING issuer, subject, scopes, token_family_id",
        )
        .bind(now.get())
        .bind(hash_token(&rotation.refresh_token))
        .bind(&rotation.client_id)
        .bind(&rotation.resource_uri)
        .bind(now.get())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database_error)?;
        let Some(row) = row else {
            let replayed_family = sqlx::query_scalar::<_, Vec<u8>>(
                "SELECT token_family_id FROM mcp_refresh_tokens
                 WHERE token_hash = ? AND client_id = ? AND resource_uri = ?
                   AND rotated_at_ms IS NOT NULL AND revoked_at_ms IS NULL",
            )
            .bind(hash_token(&rotation.refresh_token))
            .bind(&rotation.client_id)
            .bind(&rotation.resource_uri)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(database_error)?;
            if let Some(token_family_id) = replayed_family {
                for table in ["mcp_access_tokens", "mcp_refresh_tokens"] {
                    let query = format!(
                        "UPDATE {table} SET revoked_at_ms = ?
                         WHERE token_family_id = ? AND revoked_at_ms IS NULL"
                    );
                    sqlx::query(&query)
                        .bind(now.get())
                        .bind(&token_family_id)
                        .execute(&mut *transaction)
                        .await
                        .map_err(database_error)?;
                }
                transaction.commit().await.map_err(database_error)?;
            }
            return Ok(McpRefreshTokenRotationOutcome::InvalidToken);
        };
        let token_family_id = row
            .try_get::<Vec<u8>, _>("token_family_id")
            .map_err(database_error)?;
        let original_scopes = row
            .try_get::<String, _>("scopes")
            .map_err(database_error)?
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let access_scopes = rotation
            .requested_scopes
            .clone()
            .unwrap_or_else(|| original_scopes.clone());
        if !access_scopes
            .iter()
            .all(|scope| original_scopes.contains(scope))
        {
            transaction.rollback().await.map_err(database_error)?;
            return Ok(McpRefreshTokenRotationOutcome::InvalidScope);
        }
        let grant = McpAuthorizationGrant {
            actor: actor_from_row(&row)?,
            client_id: rotation.client_id,
            redirect_uri: String::new(),
            resource_uri: rotation.resource_uri,
            scopes: original_scopes,
        };
        let access_scope_value = access_scopes.join(" ");
        let refresh_scope_value = grant.scopes.join(" ");
        sqlx::query("INSERT INTO mcp_access_tokens (token_hash, client_id, resource_uri, issuer, subject, scopes, expires_at_ms, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(&rotation.new_access_token)).bind(&grant.client_id).bind(&grant.resource_uri).bind(grant.actor.issuer()).bind(grant.actor.subject()).bind(access_scope_value).bind(rotation.access_expires_at.get()).bind(&token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        sqlx::query("INSERT INTO mcp_refresh_tokens (token_hash, client_id, resource_uri, issuer, subject, scopes, expires_at_ms, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(&rotation.new_refresh_token)).bind(&grant.client_id).bind(&grant.resource_uri).bind(grant.actor.issuer()).bind(grant.actor.subject()).bind(refresh_scope_value).bind(rotation.refresh_expires_at.get()).bind(token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(McpRefreshTokenRotationOutcome::Rotated {
            grant,
            access_scopes,
        })
    }

    pub async fn revoke_mcp_client_tokens(
        &self,
        issuer: &str,
        subject: &str,
        client_id: &str,
        now: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        for table in ["mcp_access_tokens", "mcp_refresh_tokens"] {
            let query = format!(
                "UPDATE {table} SET revoked_at_ms = ? WHERE issuer = ? AND subject = ? AND client_id = ? AND revoked_at_ms IS NULL"
            );
            sqlx::query(&query)
                .bind(now.get())
                .bind(issuer)
                .bind(subject)
                .bind(client_id)
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
        }
        transaction.commit().await.map_err(database_error)
    }

    /// RFC 7009の要求に従い、指定されたtokenが属するfamily全体を失効する。
    ///
    /// 未知のtokenや別clientのtokenは情報を開示せず、成功として扱う。
    pub async fn revoke_mcp_token(
        &self,
        token: &str,
        client_id: &str,
        now: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let token_hash = hash_token(token);
        let token_family_id = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT token_family_id FROM mcp_access_tokens
             WHERE token_hash = ? AND client_id = ?
             UNION ALL
             SELECT token_family_id FROM mcp_refresh_tokens
             WHERE token_hash = ? AND client_id = ?
             LIMIT 1",
        )
        .bind(&token_hash)
        .bind(client_id)
        .bind(&token_hash)
        .bind(client_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database_error)?;
        if let Some(token_family_id) = token_family_id {
            for table in ["mcp_access_tokens", "mcp_refresh_tokens"] {
                let query = format!(
                    "UPDATE {table} SET revoked_at_ms = ?
                     WHERE token_family_id = ? AND revoked_at_ms IS NULL"
                );
                sqlx::query(&query)
                    .bind(now.get())
                    .bind(&token_family_id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(database_error)?;
            }
        }
        transaction.commit().await.map_err(database_error)
    }
}

fn actor_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Actor, SqliteStoreError> {
    Actor::try_new(
        row.try_get("issuer").map_err(database_error)?,
        row.try_get("subject").map_err(database_error)?,
    )
    .map_err(|_| SqliteStoreError::CorruptData)
}
