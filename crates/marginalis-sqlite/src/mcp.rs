//! MCP OAuth client、code、access/refresh tokenの永続化。

use marginalis_application::McpRefreshTokenRotation;
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
        sqlx::query("INSERT INTO mcp_authorization_codes (code_hash, client_id, redirect_uri, resource_uri, issuer, subject, is_administrator, scopes, code_challenge, expires_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(code)).bind(&grant.client_id).bind(&grant.redirect_uri).bind(&grant.resource_uri)
            .bind(&grant.actor.issuer).bind(&grant.actor.subject).bind(grant.actor.is_administrator)
            .bind(grant.scopes.join(" ")).bind(code_challenge).bind(expires_at.get())
            .execute(&self.pool).await.map_err(database_error)?;
        Ok(())
    }

    pub async fn upsert_mcp_client(
        &self,
        client: &McpOAuthClient,
        registered_at: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        let redirect_uris = serde_json::to_string(&client.redirect_uris)
            .map_err(|_| SqliteStoreError::CorruptNote)?;
        sqlx::query("INSERT INTO mcp_clients (client_id, display_name, redirect_uris_json, registered_at_ms) VALUES (?, ?, ?, ?) ON CONFLICT(client_id) DO UPDATE SET display_name = excluded.display_name, redirect_uris_json = excluded.redirect_uris_json")
            .bind(&client.client_id).bind(&client.display_name).bind(redirect_uris).bind(registered_at.get()).execute(&self.pool).await.map_err(database_error)?;
        Ok(())
    }

    /// Removes expired OAuth state and stale clients, then atomically registers one client only
    /// while the configured persistence bound has capacity.
    pub async fn register_mcp_client_bounded(
        &self,
        client: &McpOAuthClient,
        now: UnixMillis,
        unused_client_cutoff: UnixMillis,
        maximum_clients: i64,
    ) -> Result<bool, SqliteStoreError> {
        let redirect_uris = serde_json::to_string(&client.redirect_uris)
            .map_err(|_| SqliteStoreError::CorruptNote)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        for statement in [
            "DELETE FROM mcp_authorization_codes WHERE expires_at_ms <= ? OR consumed_at_ms IS NOT NULL",
            "DELETE FROM mcp_access_tokens WHERE expires_at_ms <= ? OR revoked_at_ms IS NOT NULL",
        ] {
            sqlx::query(statement)
                .bind(now.get())
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
        }
        sqlx::query(
            "DELETE FROM mcp_refresh_tokens AS stale
             WHERE stale.revoked_at_ms IS NOT NULL
                OR (
                    (stale.expires_at_ms <= ? OR stale.rotated_at_ms IS NOT NULL)
                    AND NOT EXISTS (
                        SELECT 1 FROM mcp_refresh_tokens AS active
                        WHERE active.token_family_id = stale.token_family_id
                          AND active.rotated_at_ms IS NULL
                          AND active.revoked_at_ms IS NULL
                          AND active.expires_at_ms > ?
                    )
                )",
        )
        .bind(now.get())
        .bind(now.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        sqlx::query(
            "DELETE FROM mcp_clients
             WHERE registered_at_ms < ?
               AND NOT EXISTS (SELECT 1 FROM mcp_authorization_codes WHERE client_id = mcp_clients.client_id)
               AND NOT EXISTS (SELECT 1 FROM mcp_access_tokens WHERE client_id = mcp_clients.client_id)
               AND NOT EXISTS (SELECT 1 FROM mcp_refresh_tokens WHERE client_id = mcp_clients.client_id)",
        )
        .bind(unused_client_cutoff.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM mcp_clients")
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;
        if count >= maximum_clients {
            transaction.commit().await.map_err(database_error)?;
            return Ok(false);
        }
        sqlx::query("INSERT INTO mcp_clients (client_id, display_name, redirect_uris_json, registered_at_ms) VALUES (?, ?, ?, ?)")
            .bind(&client.client_id)
            .bind(&client.display_name)
            .bind(redirect_uris)
            .bind(now.get())
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(true)
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
                .map_err(|_| SqliteStoreError::CorruptNote)?,
            })
        })
        .transpose()
    }

    pub async fn consume_mcp_authorization_code(
        &self,
        code: &str,
        client_id: &str,
        redirect_uri: &str,
        resource_uri: &str,
        code_challenge: &str,
        now: UnixMillis,
    ) -> Result<Option<McpAuthorizationGrant>, SqliteStoreError> {
        let row = sqlx::query("DELETE FROM mcp_authorization_codes WHERE code_hash = ? AND client_id = ? AND redirect_uri = ? AND resource_uri = ? AND code_challenge = ? AND expires_at_ms > ? RETURNING issuer, subject, is_administrator, scopes")
            .bind(hash_token(code)).bind(client_id).bind(redirect_uri).bind(resource_uri).bind(code_challenge).bind(now.get()).fetch_optional(&self.pool).await.map_err(database_error)?;
        row.map(|row| {
            Ok(McpAuthorizationGrant {
                actor: Actor {
                    issuer: row.try_get("issuer").map_err(database_error)?,
                    subject: row.try_get("subject").map_err(database_error)?,
                    is_administrator: row.try_get("is_administrator").map_err(database_error)?,
                },
                client_id: client_id.into(),
                redirect_uri: redirect_uri.into(),
                resource_uri: resource_uri.into(),
                scopes: row
                    .try_get::<String, _>("scopes")
                    .map_err(database_error)?
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect(),
            })
        })
        .transpose()
    }

    pub async fn issue_mcp_token_pair(
        &self,
        access_token: &str,
        refresh_token: &str,
        grant: &McpAuthorizationGrant,
        access_expires_at: UnixMillis,
        refresh_expires_at: UnixMillis,
        _now: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let scopes = grant.scopes.join(" ");
        let token_family_id = hash_token(refresh_token);
        sqlx::query("INSERT INTO mcp_access_tokens (token_hash, client_id, resource_uri, issuer, subject, is_administrator, scopes, expires_at_ms, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(access_token)).bind(&grant.client_id).bind(&grant.resource_uri).bind(&grant.actor.issuer).bind(&grant.actor.subject).bind(grant.actor.is_administrator).bind(&scopes).bind(access_expires_at.get()).bind(&token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        sqlx::query("INSERT INTO mcp_refresh_tokens (token_hash, client_id, resource_uri, issuer, subject, scopes, expires_at_ms, is_administrator, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(refresh_token)).bind(&grant.client_id).bind(&grant.resource_uri).bind(&grant.actor.issuer).bind(&grant.actor.subject).bind(scopes).bind(refresh_expires_at.get()).bind(grant.actor.is_administrator).bind(token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        transaction.commit().await.map_err(database_error)
    }

    pub async fn authenticate_mcp_access_token(
        &self,
        token: &str,
        resource_uri: &str,
        scope: &str,
        now: UnixMillis,
    ) -> Result<Option<McpAuthenticatedActor>, SqliteStoreError> {
        let row = sqlx::query("SELECT issuer, subject, is_administrator FROM mcp_access_tokens WHERE token_hash = ? AND resource_uri = ? AND revoked_at_ms IS NULL AND expires_at_ms > ? AND instr(' ' || scopes || ' ', ' ' || ? || ' ') > 0")
            .bind(hash_token(token)).bind(resource_uri).bind(now.get()).bind(scope).fetch_optional(&self.pool).await.map_err(database_error)?;
        row.map(|r| {
            Ok(McpAuthenticatedActor {
                actor: Actor {
                    issuer: r.try_get("issuer").map_err(database_error)?,
                    subject: r.try_get("subject").map_err(database_error)?,
                    is_administrator: r.try_get("is_administrator").map_err(database_error)?,
                },
            })
        })
        .transpose()
    }

    /// refresh tokenを一度だけ消費し、同じKanidm主体に新しいtoken pairを発行する。
    pub async fn rotate_mcp_refresh_token(
        &self,
        rotation: McpRefreshTokenRotation,
        now: UnixMillis,
    ) -> Result<Option<McpAuthorizationGrant>, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let row = sqlx::query(
            "UPDATE mcp_refresh_tokens SET rotated_at_ms = ?
             WHERE token_hash = ? AND client_id = ? AND resource_uri = ?
               AND rotated_at_ms IS NULL AND revoked_at_ms IS NULL AND expires_at_ms > ?
             RETURNING issuer, subject, is_administrator, scopes, token_family_id",
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
            return Ok(None);
        };
        let token_family_id = row
            .try_get::<Vec<u8>, _>("token_family_id")
            .map_err(database_error)?;
        let grant = McpAuthorizationGrant {
            actor: Actor {
                issuer: row.try_get("issuer").map_err(database_error)?,
                subject: row.try_get("subject").map_err(database_error)?,
                is_administrator: row.try_get("is_administrator").map_err(database_error)?,
            },
            client_id: rotation.client_id,
            redirect_uri: String::new(),
            resource_uri: rotation.resource_uri,
            scopes: row
                .try_get::<String, _>("scopes")
                .map_err(database_error)?
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        };
        let scopes = grant.scopes.join(" ");
        sqlx::query("INSERT INTO mcp_access_tokens (token_hash, client_id, resource_uri, issuer, subject, is_administrator, scopes, expires_at_ms, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(&rotation.new_access_token)).bind(&grant.client_id).bind(&grant.resource_uri).bind(&grant.actor.issuer).bind(&grant.actor.subject).bind(grant.actor.is_administrator).bind(&scopes).bind(rotation.access_expires_at.get()).bind(&token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        sqlx::query("INSERT INTO mcp_refresh_tokens (token_hash, client_id, resource_uri, issuer, subject, scopes, expires_at_ms, is_administrator, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(&rotation.new_refresh_token)).bind(&grant.client_id).bind(&grant.resource_uri).bind(&grant.actor.issuer).bind(&grant.actor.subject).bind(scopes).bind(rotation.refresh_expires_at.get()).bind(grant.actor.is_administrator).bind(token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(Some(grant))
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
}
