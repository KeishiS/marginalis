//! MCP OAuth client、code、access/refresh tokenの永続化。

use marginalis_domain::Actor;
use mcp_authorization_server::{
    AuthenticatedPrincipal as McpAuthenticatedPrincipal,
    AuthorizationCodeExchange as McpAuthorizationCodeExchange,
    AuthorizationGrant as McpAuthorizationGrant, Client as McpOAuthClient,
    ClientRegistrationMethod as McpClientRegistrationMethod, Principal as McpPrincipal,
    RefreshTokenRotation as McpRefreshTokenRotation,
    RefreshTokenRotationOutcome as McpRefreshTokenRotationOutcome,
    RegisteredClient as McpRegisteredOAuthClient, ResolvedRedirectUri as McpResolvedRedirectUri,
    Timestamp as McpTimestamp,
};
use sqlx::Row;

use crate::{SqliteDatabase, SqliteStoreError, database_error, token::hash_token};

impl SqliteDatabase {
    /// 同意した認可codeを、その時点のclient登録と一緒に保存する。
    ///
    /// Client ID Metadata Documentで解決したclientは事前登録がなく、`mcp_clients`への外部key
    /// を満たせない。clientの登録とcodeの保存を一つのtransactionで行い、間に定期削除が
    /// 割り込んでもcodeが宙に浮かないようにする。
    pub async fn issue_mcp_authorization_code(
        &self,
        code: &str,
        registered_client: &McpRegisteredOAuthClient,
        grant: &McpAuthorizationGrant,
        code_challenge: &str,
        expires_at: McpTimestamp,
        now: McpTimestamp,
    ) -> Result<(), SqliteStoreError> {
        let client = &registered_client.client;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        upsert_client(
            &mut *transaction,
            client,
            registered_client.registration_method,
            now,
        )
        .await?;
        sqlx::query("INSERT INTO mcp_authorization_codes (code_hash, client_id, redirect_uri, redirect_uri_was_supplied, resource_uri, issuer, subject, scopes, code_challenge, expires_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(code)).bind(&client.client_id).bind(grant.redirect_uri.as_str()).bind(grant.redirect_uri.was_supplied()).bind(&grant.resource_uri)
            .bind(grant.principal.issuer()).bind(grant.principal.subject())
            .bind(grant.scopes.join(" ")).bind(code_challenge).bind(expires_at.get())
            .execute(&mut *transaction).await.map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn upsert_mcp_client(
        &self,
        client: &McpOAuthClient,
        registered_at: McpTimestamp,
    ) -> Result<(), SqliteStoreError> {
        upsert_client(
            &self.pool,
            client,
            McpClientRegistrationMethod::Dynamic,
            registered_at,
        )
        .await
    }

    /// configured persistence boundに空きがある場合だけclientを原子的に登録する。
    pub async fn register_mcp_client_bounded(
        &self,
        client: &McpOAuthClient,
        now: McpTimestamp,
        maximum_clients: i64,
    ) -> Result<bool, SqliteStoreError> {
        let redirect_uris = serde_json::to_string(&client.redirect_uris)
            .map_err(|_| SqliteStoreError::CorruptData)?;
        let result = sqlx::query(
            "INSERT INTO mcp_clients
                 (client_id, display_name, redirect_uris_json, registration_method, registered_at_ms)
             SELECT ?, ?, ?, 'dynamic', ?
             WHERE (SELECT COUNT(*) FROM mcp_clients WHERE registration_method = 'dynamic') < ?",
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

    #[cfg(test)]
    pub(crate) async fn mcp_client(
        &self,
        client_id: &str,
    ) -> Result<Option<McpOAuthClient>, SqliteStoreError> {
        Ok(self
            .registered_mcp_client(client_id)
            .await?
            .map(|registered| registered.client))
    }

    pub async fn registered_mcp_client(
        &self,
        client_id: &str,
    ) -> Result<Option<McpRegisteredOAuthClient>, SqliteStoreError> {
        let row = sqlx::query("SELECT client_id, display_name, redirect_uris_json, registration_method FROM mcp_clients WHERE client_id = ?")
            .bind(client_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        row.map(registered_client_from_row).transpose()
    }

    /// 認可codeを一度だけ消費してtoken pairを発行し、再利用時は発行済みfamilyを失効する。
    ///
    /// 認可要求でredirect URIが指定されていた場合は、token要求でも同じ値を必須とする。
    /// 省略されていた場合は、token要求での省略または解決済みの同じ値を受け付ける。
    pub async fn exchange_mcp_authorization_code(
        &self,
        exchange: McpAuthorizationCodeExchange,
        now: McpTimestamp,
    ) -> Result<Option<McpAuthorizationGrant>, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let code_hash = hash_token(&exchange.code);
        let row = sqlx::query(
            "UPDATE mcp_authorization_codes SET consumed_at_ms = ?
             WHERE code_hash = ? AND client_id = ?
               AND ((redirect_uri_was_supplied = 1 AND redirect_uri = ?)
                    OR (redirect_uri_was_supplied = 0 AND (? IS NULL OR redirect_uri = ?)))
               AND resource_uri = ? AND code_challenge = ?
               AND consumed_at_ms IS NULL AND expires_at_ms > ?
             RETURNING redirect_uri, redirect_uri_was_supplied, issuer, subject, scopes",
        )
        .bind(now.get())
        .bind(&code_hash)
        .bind(&exchange.client_id)
        .bind(exchange.redirect_uri.as_deref())
        .bind(exchange.redirect_uri.as_deref())
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
                   AND ((redirect_uri_was_supplied = 1 AND redirect_uri = ?)
                        OR (redirect_uri_was_supplied = 0 AND (? IS NULL OR redirect_uri = ?)))
                   AND resource_uri = ? AND code_challenge = ?
                   AND consumed_at_ms IS NOT NULL
                   AND token_family_id IS NOT NULL",
            )
            .bind(&code_hash)
            .bind(&exchange.client_id)
            .bind(exchange.redirect_uri.as_deref())
            .bind(exchange.redirect_uri.as_deref())
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
        let redirect_uri = row
            .try_get::<String, _>("redirect_uri")
            .map_err(database_error)?;
        let redirect_uri = if row
            .try_get::<bool, _>("redirect_uri_was_supplied")
            .map_err(database_error)?
        {
            McpResolvedRedirectUri::Supplied(redirect_uri)
        } else {
            McpResolvedRedirectUri::Inferred(redirect_uri)
        };
        let grant = McpAuthorizationGrant {
            principal: principal_from_row(&row)?,
            client_id: exchange.client_id,
            redirect_uri,
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
            .bind(hash_token(&exchange.access_token)).bind(&grant.client_id).bind(&grant.resource_uri).bind(grant.principal.issuer()).bind(grant.principal.subject()).bind(&scopes).bind(exchange.access_expires_at.get()).bind(&token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        sqlx::query("INSERT INTO mcp_refresh_tokens (token_hash, client_id, resource_uri, issuer, subject, scopes, expires_at_ms, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(&exchange.refresh_token)).bind(&grant.client_id).bind(&grant.resource_uri).bind(grant.principal.issuer()).bind(grant.principal.subject()).bind(scopes).bind(exchange.refresh_expires_at.get()).bind(&token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
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
        now: McpTimestamp,
    ) -> Result<Option<McpAuthenticatedPrincipal>, SqliteStoreError> {
        let row = sqlx::query("SELECT issuer, subject, scopes FROM mcp_access_tokens WHERE token_hash = ? AND resource_uri = ? AND revoked_at_ms IS NULL AND expires_at_ms > ?")
            .bind(hash_token(token)).bind(resource_uri).bind(now.get()).fetch_optional(&self.pool).await.map_err(database_error)?;
        row.map(|r| {
            Ok(McpAuthenticatedPrincipal {
                principal: principal_from_row(&r)?,
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
        now: McpTimestamp,
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
        let issuer = row.try_get::<String, _>("issuer").map_err(database_error)?;
        let subject = row
            .try_get::<String, _>("subject")
            .map_err(database_error)?;
        let access_scope_value = access_scopes.join(" ");
        let refresh_scope_value = original_scopes.join(" ");
        sqlx::query("INSERT INTO mcp_access_tokens (token_hash, client_id, resource_uri, issuer, subject, scopes, expires_at_ms, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(&rotation.new_access_token)).bind(&rotation.client_id).bind(&rotation.resource_uri).bind(&issuer).bind(&subject).bind(access_scope_value).bind(rotation.access_expires_at.get()).bind(&token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        sqlx::query("INSERT INTO mcp_refresh_tokens (token_hash, client_id, resource_uri, issuer, subject, scopes, expires_at_ms, token_family_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(hash_token(&rotation.new_refresh_token)).bind(&rotation.client_id).bind(&rotation.resource_uri).bind(issuer).bind(subject).bind(refresh_scope_value).bind(rotation.refresh_expires_at.get()).bind(token_family_id).execute(&mut *transaction).await.map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(McpRefreshTokenRotationOutcome::Rotated { access_scopes })
    }

    pub async fn revoke_mcp_client_tokens(
        &self,
        issuer: &str,
        subject: &str,
        client_id: &str,
        now: McpTimestamp,
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
        now: McpTimestamp,
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

/// clientを登録する。既存clientは表示名とredirect URIだけを更新する。
///
/// `registered_at_ms`は最初に登録した時刻のまま残す。認可のたびに更新すると、使われなくなった
/// clientを定期削除が回収できなくなるためである。
async fn upsert_client<'e, E>(
    executor: E,
    client: &McpOAuthClient,
    registration_method: McpClientRegistrationMethod,
    registered_at: McpTimestamp,
) -> Result<(), SqliteStoreError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let redirect_uris =
        serde_json::to_string(&client.redirect_uris).map_err(|_| SqliteStoreError::CorruptData)?;
    let result = sqlx::query("INSERT INTO mcp_clients (client_id, display_name, redirect_uris_json, registration_method, registered_at_ms) VALUES (?, ?, ?, ?, ?) ON CONFLICT(client_id) DO UPDATE SET display_name = excluded.display_name, redirect_uris_json = excluded.redirect_uris_json WHERE mcp_clients.registration_method = excluded.registration_method")
        .bind(&client.client_id)
        .bind(&client.display_name)
        .bind(redirect_uris)
        .bind(registration_method_value(registration_method))
        .bind(registered_at.get())
        .execute(executor)
        .await
        .map_err(database_error)?;
    if result.rows_affected() != 1 {
        return Err(SqliteStoreError::CorruptData);
    }
    Ok(())
}

fn registered_client_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<McpRegisteredOAuthClient, SqliteStoreError> {
    let registration_method = match row
        .try_get::<String, _>("registration_method")
        .map_err(database_error)?
        .as_str()
    {
        "dynamic" => McpClientRegistrationMethod::Dynamic,
        "metadata_document" => McpClientRegistrationMethod::MetadataDocument,
        _ => return Err(SqliteStoreError::CorruptData),
    };
    Ok(McpRegisteredOAuthClient {
        client: McpOAuthClient {
            client_id: row.try_get("client_id").map_err(database_error)?,
            display_name: row.try_get("display_name").map_err(database_error)?,
            redirect_uris: serde_json::from_str(
                &row.try_get::<String, _>("redirect_uris_json")
                    .map_err(database_error)?,
            )
            .map_err(|_| SqliteStoreError::CorruptData)?,
        },
        registration_method,
    })
}

const fn registration_method_value(method: McpClientRegistrationMethod) -> &'static str {
    match method {
        McpClientRegistrationMethod::Dynamic => "dynamic",
        McpClientRegistrationMethod::MetadataDocument => "metadata_document",
    }
}

fn principal_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<McpPrincipal, SqliteStoreError> {
    let actor = Actor::try_new(
        row.try_get("issuer").map_err(database_error)?,
        row.try_get("subject").map_err(database_error)?,
    )
    .map_err(|_| SqliteStoreError::CorruptData)?;
    Ok(McpPrincipal::new(
        actor.issuer().into(),
        actor.subject().into(),
    ))
}
