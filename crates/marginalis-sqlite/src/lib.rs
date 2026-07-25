//! Marginalisの現行データモデルに限定したSQLite adapter。

use std::{collections::HashSet, fmt, future::Future, str::FromStr, time::Duration};

use marginalis_application::{
    McpRefreshTokenRotation, OidcLoginAttempt, OidcLoginAttemptStore,
};
use marginalis_domain::{
    ARCHIVE_FORMAT, Actor, Archive,
    AuthenticatedSession, McpAuthenticatedActor,
    McpAuthorizationGrant, Note, NoteAclEntry,
    NoteBundle, NoteDraft, WebSession, EntityId,
    McpOAuthClient, NoteId, NotePermission, UnixMillis,
};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, Sqlite, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

const SCHEMA_VERSION: i64 = 1;
const INITIAL_SCHEMA: &str = r#"
CREATE TABLE notes (
    note_id TEXT PRIMARY KEY NOT NULL,
    creator_issuer TEXT NOT NULL,
    creator_subject TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    tags_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    deleted_at_ms INTEGER
) STRICT;

CREATE TABLE note_acl (
    note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    permission INTEGER NOT NULL CHECK (permission BETWEEN 1 AND 3),
    PRIMARY KEY (note_id, issuer, subject)
) STRICT;

CREATE TABLE note_references (
    source_note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    source_start INTEGER NOT NULL CHECK (source_start >= 0),
    source_end INTEGER NOT NULL CHECK (source_end > source_start),
    target_note_id TEXT NOT NULL,
    target_anchor TEXT,
    PRIMARY KEY (source_note_id, source_start, source_end)
) STRICT;

CREATE TABLE note_anchors (
    note_id TEXT NOT NULL REFERENCES notes(note_id) ON DELETE CASCADE,
    anchor_id TEXT NOT NULL,
    PRIMARY KEY (note_id, anchor_id)
) STRICT;

CREATE VIRTUAL TABLE note_search USING fts5(
    note_id UNINDEXED,
    title,
    body
);

CREATE TABLE web_sessions (
    session_id_hash BLOB PRIMARY KEY NOT NULL,
    csrf_token_hash BLOB NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    is_administrator INTEGER NOT NULL CHECK (is_administrator IN (0, 1)),
    issued_at_ms INTEGER NOT NULL,
    last_seen_at_ms INTEGER NOT NULL,
    idle_expires_at_ms INTEGER NOT NULL,
    absolute_expires_at_ms INTEGER NOT NULL,
    revoked_at_ms INTEGER
) STRICT;
CREATE INDEX web_sessions_subject_idx
ON web_sessions (issuer, subject)
WHERE revoked_at_ms IS NULL;

CREATE TABLE oidc_login_attempts (
    state_hash BLOB PRIMARY KEY NOT NULL,
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE mcp_clients (
    client_id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    redirect_uris_json TEXT NOT NULL,
    registered_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE mcp_authorization_codes (
    code_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    redirect_uri TEXT NOT NULL,
    resource_uri TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    is_administrator INTEGER NOT NULL CHECK (is_administrator IN (0, 1)),
    scopes TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    consumed_at_ms INTEGER
) STRICT;

CREATE TABLE mcp_access_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    resource_uri TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    is_administrator INTEGER NOT NULL CHECK (is_administrator IN (0, 1)),
    scopes TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    revoked_at_ms INTEGER,
    last_used_at_ms INTEGER,
    token_family_id BLOB NOT NULL CHECK (length(token_family_id) = 32)
) STRICT;
CREATE INDEX mcp_access_subject_idx
ON mcp_access_tokens (issuer, subject)
WHERE revoked_at_ms IS NULL;

CREATE TABLE mcp_refresh_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL REFERENCES mcp_clients(client_id),
    resource_uri TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    scopes TEXT NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    rotated_at_ms INTEGER,
    revoked_at_ms INTEGER,
    is_administrator INTEGER NOT NULL CHECK (is_administrator IN (0, 1)),
    token_family_id BLOB NOT NULL CHECK (length(token_family_id) = 32)
) STRICT;
CREATE INDEX mcp_refresh_family_idx ON mcp_refresh_tokens (token_family_id);
"#;

#[derive(Clone, Debug)]
pub struct SqliteDatabase {
    pool: SqlitePool,
}

#[derive(Clone, Debug)]
pub struct SqliteOidcLoginAttemptStore {
    pool: SqlitePool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SqliteStoreError {
    Conflict,
    LastAdmin,
    ArchiveFormat,
    ArchiveTargetNotEmpty,
    ArchiveMissingAdmin,
    CorruptNote,
    Database(String),
}

impl fmt::Display for SqliteStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict => formatter.write_str("note revision does not match"),
            Self::LastAdmin => formatter.write_str("a note must retain one direct administrator"),
            Self::ArchiveFormat => formatter.write_str("archive format is unsupported"),
            Self::ArchiveTargetNotEmpty => {
                formatter.write_str("archive import target must be empty")
            }
            Self::ArchiveMissingAdmin => {
                formatter.write_str("every archived note must retain one direct administrator")
            }
            Self::CorruptNote => formatter.write_str("note data is invalid"),
            Self::Database(_) => formatter.write_str("database query failed"),
        }
    }
}

impl std::error::Error for SqliteStoreError {}

impl SqliteDatabase {
    /// v0.3.0専用のSQLite schemaへ接続する。
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let options = database_url
            .parse::<SqliteConnectOptions>()?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        migrate(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn oidc_login_attempt_store(&self) -> SqliteOidcLoginAttemptStore {
        SqliteOidcLoginAttemptStore {
            pool: self.pool.clone(),
        }
    }

    /// Web sessionの不透明値はhashだけを保存する。
    pub async fn issue_web_session(
        &self,
        session: &WebSession,
        now: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        sqlx::query(
            "INSERT INTO web_sessions
             (session_id_hash, csrf_token_hash, issuer, subject, is_administrator,
              issued_at_ms, last_seen_at_ms, idle_expires_at_ms, absolute_expires_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(hash_token(&session.session_id))
        .bind(hash_token(&session.csrf_token))
        .bind(&session.actor.issuer)
        .bind(&session.actor.subject)
        .bind(session.actor.is_administrator)
        .bind(now.get())
        .bind(now.get())
        .bind(session.idle_expires_at.get())
        .bind(session.absolute_expires_at.get())
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    /// sessionの期限を検証し、活動中なら利用時刻だけを更新する。
    pub async fn lookup_web_session(
        &self,
        session_id: &str,
        now: UnixMillis,
    ) -> Result<Option<AuthenticatedSession>, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let hash = hash_token(session_id);
        let row = sqlx::query(
            "SELECT issuer, subject, is_administrator, idle_expires_at_ms, absolute_expires_at_ms
             FROM web_sessions WHERE session_id_hash = ? AND revoked_at_ms IS NULL",
        )
        .bind(&hash)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let session = session_from_row(row)?;
        if session.idle_expires_at <= now || session.absolute_expires_at <= now {
            sqlx::query(
                "UPDATE web_sessions SET revoked_at_ms = ? WHERE session_id_hash = ? AND revoked_at_ms IS NULL",
            )
            .bind(now.get())
            .bind(hash)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
            transaction.commit().await.map_err(database_error)?;
            return Ok(None);
        }
        sqlx::query("UPDATE web_sessions SET last_seen_at_ms = ? WHERE session_id_hash = ?")
            .bind(now.get())
            .bind(hash)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(Some(session))
    }

    pub async fn validate_web_session_csrf(
        &self,
        session_id: &str,
        csrf_token: &str,
    ) -> Result<bool, SqliteStoreError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM web_sessions
             WHERE session_id_hash = ? AND csrf_token_hash = ? AND revoked_at_ms IS NULL",
        )
        .bind(hash_token(session_id))
        .bind(hash_token(csrf_token))
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?
        .is_some();
        Ok(exists)
    }

    pub async fn revoke_web_session(
        &self,
        session_id: &str,
        now: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        sqlx::query("UPDATE web_sessions SET revoked_at_ms = ? WHERE session_id_hash = ? AND revoked_at_ms IS NULL")
            .bind(now.get()).bind(hash_token(session_id)).execute(&self.pool).await.map_err(database_error)?;
        Ok(())
    }

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

    /// 正本、直接ACL、検索投影を同一transactionで作成する。
    pub async fn create_note(
        &self,
        note: &Note,
        owner_permission: NotePermission,
    ) -> Result<(), SqliteStoreError> {
        let tags_json =
            serde_json::to_string(&note.tags).map_err(|_| SqliteStoreError::CorruptNote)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        sqlx::query(
            "INSERT INTO notes (note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(note.note_id.to_string())
        .bind(&note.creator_issuer)
        .bind(&note.creator_subject)
        .bind(&note.title)
        .bind(&note.body)
        .bind(tags_json)
        .bind(note.created_at.get())
        .bind(note.updated_at.get())
        .bind(note.revision)
        .bind(note.deleted_at.map(UnixMillis::get))
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        sqlx::query(
            "INSERT INTO note_acl (note_id, issuer, subject, permission) VALUES (?, ?, ?, ?)",
        )
        .bind(note.note_id.to_string())
        .bind(&note.creator_issuer)
        .bind(&note.creator_subject)
        .bind(permission_to_storage(owner_permission))
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        insert_search_row(&mut transaction, note).await?;
        transaction.commit().await.map_err(database_error)
    }

    pub async fn note(
        &self,
        note_id: NoteId,
        include_deleted: bool,
    ) -> Result<Option<Note>, SqliteStoreError> {
        let row = if include_deleted {
            sqlx::query(
                "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
                 FROM notes WHERE note_id = ?",
            )
            .bind(note_id.to_string())
            .fetch_optional(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
                 FROM notes WHERE note_id = ? AND deleted_at_ms IS NULL",
            )
            .bind(note_id.to_string())
            .fetch_optional(&self.pool)
            .await
        }
        .map_err(database_error)?;
        row.map(note_from_row).transpose()
    }

    /// 管理者または直接ACLを持つ主体だけに、削除済みでない正本を返す。
    pub async fn visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        required: NotePermission,
    ) -> Result<Option<Note>, SqliteStoreError> {
        let row = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes
             WHERE note_id = ? AND deleted_at_ms IS NULL
               AND (? OR EXISTS (
                    SELECT 1 FROM note_acl
                    WHERE note_acl.note_id = notes.note_id
                      AND issuer = ? AND subject = ? AND permission >= ?
               ))",
        )
        .bind(note_id.to_string())
        .bind(actor.is_administrator)
        .bind(&actor.issuer)
        .bind(&actor.subject)
        .bind(permission_to_storage(required))
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        row.map(note_from_row).transpose()
    }

    /// 復元候補として削除済みノートをAdminだけへ返す。
    pub async fn visible_deleted_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Note>, SqliteStoreError> {
        let row = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes
             WHERE note_id = ? AND deleted_at_ms IS NOT NULL
               AND (? OR EXISTS (
                    SELECT 1 FROM note_acl
                    WHERE note_acl.note_id = notes.note_id
                      AND issuer = ? AND subject = ? AND permission >= ?
               ))",
        )
        .bind(note_id.to_string())
        .bind(actor.is_administrator)
        .bind(&actor.issuer)
        .bind(&actor.subject)
        .bind(permission_to_storage(NotePermission::Admin))
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        row.map(note_from_row).transpose()
    }

    /// 削除済みでない、主体に可視なノートを安定した順序で返す。
    pub async fn list_visible_notes(
        &self,
        actor: &Actor,
        offset: u64,
        limit: u32,
    ) -> Result<Vec<Note>, SqliteStoreError> {
        let rows = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes
             WHERE deleted_at_ms IS NULL
               AND (? OR EXISTS (
                    SELECT 1 FROM note_acl
                    WHERE note_acl.note_id = notes.note_id
                      AND issuer = ? AND subject = ? AND permission >= ?
               ))
             ORDER BY updated_at_ms DESC, note_id ASC LIMIT ? OFFSET ?",
        )
        .bind(actor.is_administrator)
        .bind(&actor.issuer)
        .bind(&actor.subject)
        .bind(permission_to_storage(NotePermission::Read))
        .bind(i64::from(limit))
        .bind(i64::try_from(offset).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        rows.into_iter().map(note_from_row).collect()
    }

    /// 削除済みでない正本を楽観的ロックして更新する。
    pub async fn update_note(
        &self,
        note_id: NoteId,
        expected_revision: i64,
        draft: &NoteDraft,
        updated_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let tags_json =
            serde_json::to_string(&draft.tags).map_err(|_| SqliteStoreError::CorruptNote)?;
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let result = sqlx::query(
            "UPDATE notes
             SET title = ?, body = ?, tags_json = ?, updated_at_ms = ?, revision = revision + 1
             WHERE note_id = ? AND revision = ? AND deleted_at_ms IS NULL",
        )
        .bind(&draft.title)
        .bind(&draft.body)
        .bind(tags_json)
        .bind(updated_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision)
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            return Err(SqliteStoreError::Conflict);
        }
        let row = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes WHERE note_id = ?",
        )
        .bind(note_id.to_string())
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error)?;
        let note = note_from_row(row)?;
        sqlx::query("DELETE FROM note_search WHERE note_id = ?")
            .bind(note_id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        insert_search_row(&mut transaction, &note).await?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }

    /// ノートを通常の参照・検索から除外し、30日間の復元候補にする。
    pub async fn soft_delete_note(
        &self,
        note_id: NoteId,
        expected_revision: i64,
        deleted_at: UnixMillis,
    ) -> Result<(), SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let result = sqlx::query(
            "UPDATE notes SET deleted_at_ms = ?, updated_at_ms = ?, revision = revision + 1
             WHERE note_id = ? AND revision = ? AND deleted_at_ms IS NULL",
        )
        .bind(deleted_at.get())
        .bind(deleted_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision)
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            return Err(SqliteStoreError::Conflict);
        }
        sqlx::query("DELETE FROM note_search WHERE note_id = ?")
            .bind(note_id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)
    }

    /// 削除から30日以内のノートを復元する。
    pub async fn restore_note(
        &self,
        note_id: NoteId,
        expected_revision: i64,
        restored_at: UnixMillis,
    ) -> Result<Note, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let result = sqlx::query(
            "UPDATE notes SET deleted_at_ms = NULL, updated_at_ms = ?, revision = revision + 1
             WHERE note_id = ? AND revision = ? AND deleted_at_ms IS NOT NULL",
        )
        .bind(restored_at.get())
        .bind(note_id.to_string())
        .bind(expected_revision)
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            return Err(SqliteStoreError::Conflict);
        }
        let row = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes WHERE note_id = ?",
        )
        .bind(note_id.to_string())
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error)?;
        let note = note_from_row(row)?;
        insert_search_row(&mut transaction, &note).await?;
        transaction.commit().await.map_err(database_error)?;
        Ok(note)
    }

    /// retention期限を過ぎた削除済みノートを物理削除する。
    pub async fn purge_deleted_before(&self, cutoff: UnixMillis) -> Result<u64, SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        sqlx::query(
            "DELETE FROM note_search WHERE note_id IN
             (SELECT note_id FROM notes WHERE deleted_at_ms IS NOT NULL AND deleted_at_ms < ?)",
        )
        .bind(cutoff.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        let result = sqlx::query(
            "DELETE FROM notes WHERE deleted_at_ms IS NOT NULL AND deleted_at_ms < ?",
        )
        .bind(cutoff.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(result.rows_affected())
    }

    /// 直接ACLを置き換える。最後の直接Adminの降格・削除は同じtransactionで拒否する。
    pub async fn set_note_permission(
        &self,
        note_id: NoteId,
        issuer: &str,
        subject: &str,
        permission: Option<NotePermission>,
    ) -> Result<(), SqliteStoreError> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let current_permission = sqlx::query_scalar::<_, i64>(
            "SELECT permission FROM note_acl WHERE note_id = ? AND issuer = ? AND subject = ?",
        )
        .bind(note_id.to_string())
        .bind(issuer)
        .bind(subject)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(database_error)?;
        let current_is_admin = matches!(current_permission, Some(3));
        if current_is_admin && permission != Some(NotePermission::Admin) {
            let administrator_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM note_acl WHERE note_id = ? AND permission = 3",
            )
            .bind(note_id.to_string())
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;
            if administrator_count <= 1 {
                return Err(SqliteStoreError::LastAdmin);
            }
        }
        match permission {
            Some(permission) => {
                sqlx::query(
                    "INSERT INTO note_acl (note_id, issuer, subject, permission) VALUES (?, ?, ?, ?)
                     ON CONFLICT (note_id, issuer, subject) DO UPDATE SET permission = excluded.permission",
                )
                .bind(note_id.to_string())
                .bind(issuer)
                .bind(subject)
                .bind(permission_to_storage(permission))
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
            }
            None => {
                sqlx::query(
                    "DELETE FROM note_acl WHERE note_id = ? AND issuer = ? AND subject = ?",
                )
                .bind(note_id.to_string())
                .bind(issuer)
                .bind(subject)
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
            }
        }
        transaction.commit().await.map_err(database_error)
    }

    pub async fn note_acl(
        &self,
        note_id: NoteId,
    ) -> Result<Vec<NoteAclEntry>, SqliteStoreError> {
        let rows = sqlx::query(
            "SELECT issuer, subject, permission FROM note_acl
             WHERE note_id = ? ORDER BY issuer ASC, subject ASC",
        )
        .bind(note_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        rows.into_iter()
            .map(|row| {
                let permission = row
                    .try_get::<i64, _>("permission")
                    .map_err(database_error)
                    .and_then(permission_from_storage)?;
                Ok(NoteAclEntry {
                    issuer: row.try_get("issuer").map_err(database_error)?,
                    subject: row.try_get("subject").map_err(database_error)?,
                    permission,
                })
            })
            .collect()
    }

    /// SQLite正本を可搬 archive の論理表現として取り出す。
    pub async fn export_archive(&self) -> Result<Archive, SqliteStoreError> {
        let rows = sqlx::query(
            "SELECT note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms
             FROM notes ORDER BY note_id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        let mut notes = Vec::with_capacity(rows.len());
        for row in rows {
            let note = note_from_row(row)?;
            let acl = self.note_acl(note.note_id).await?;
            notes.push(NoteBundle { note, acl });
        }
        Ok(Archive {
            format: ARCHIVE_FORMAT.into(),
            notes,
        })
    }

    /// 検証済みarchiveを空の v0.3.0 databaseへ一つのtransactionでimportする。
    pub async fn import_archive(&self, archive: &Archive) -> Result<(), SqliteStoreError> {
        if archive.format != ARCHIVE_FORMAT {
            return Err(SqliteStoreError::ArchiveFormat);
        }
        let mut note_ids = HashSet::new();
        for bundle in &archive.notes {
            if !note_ids.insert(bundle.note.note_id) {
                return Err(SqliteStoreError::CorruptNote);
            }
            if EntityId::from_str(&bundle.note.note_id.to_string()).is_err()
                || bundle.note.creator_issuer.trim().is_empty()
                || bundle.note.creator_subject.trim().is_empty()
                || bundle.note.created_at > bundle.note.updated_at
                || bundle
                    .note
                    .deleted_at
                    .is_some_and(|deleted_at| deleted_at < bundle.note.created_at)
                || bundle.note.revision <= 0
            {
                return Err(SqliteStoreError::CorruptNote);
            }
            let normalized =
                marginalis_asciidoc::validate_note_draft(NoteDraft {
                    title: bundle.note.title.clone(),
                    body: bundle.note.body.clone(),
                    tags: bundle.note.tags.clone(),
                })
                .map_err(|_| SqliteStoreError::CorruptNote)?;
            if normalized.title != bundle.note.title
                || normalized.body != bundle.note.body
                || normalized.tags != bundle.note.tags
            {
                return Err(SqliteStoreError::CorruptNote);
            }
            let mut acl_subjects = HashSet::new();
            for entry in &bundle.acl {
                if entry.issuer.trim().is_empty()
                    || entry.subject.trim().is_empty()
                    || !acl_subjects.insert((&entry.issuer, &entry.subject))
                {
                    return Err(SqliteStoreError::CorruptNote);
                }
            }
            if !bundle
                .acl
                .iter()
                .any(|entry| entry.permission == NotePermission::Admin)
            {
                return Err(SqliteStoreError::ArchiveMissingAdmin);
            }
        }

        let mut transaction = self.pool.begin().await.map_err(database_error)?;
        let existing_notes = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM notes")
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;
        if existing_notes != 0 {
            return Err(SqliteStoreError::ArchiveTargetNotEmpty);
        }
        for bundle in &archive.notes {
            insert_note_row(&mut transaction, &bundle.note).await?;
            for entry in &bundle.acl {
                sqlx::query(
                    "INSERT INTO note_acl (note_id, issuer, subject, permission) VALUES (?, ?, ?, ?)",
                )
                .bind(bundle.note.note_id.to_string())
                .bind(&entry.issuer)
                .bind(&entry.subject)
                .bind(permission_to_storage(entry.permission))
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
            }
            if bundle.note.deleted_at.is_none() {
                insert_search_row(&mut transaction, &bundle.note).await?;
            }
        }
        transaction.commit().await.map_err(database_error)
    }
}


async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY NOT NULL) STRICT",
    )
    .execute(&mut *transaction)
    .await?;
    let version = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
    )
    .fetch_one(&mut *transaction)
    .await?;
    if version == 0 {
        sqlx::raw_sql(INITIAL_SCHEMA)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("INSERT INTO schema_migrations (version) VALUES (?)")
            .bind(SCHEMA_VERSION)
            .execute(&mut *transaction)
            .await?;
    } else if version != SCHEMA_VERSION {
        return Err(sqlx::Error::Protocol(format!(
            "unsupported database schema version {version}; expected {SCHEMA_VERSION}"
        )));
    }
    transaction.commit().await
}

async fn insert_search_row(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    note: &Note,
) -> Result<(), SqliteStoreError> {
    sqlx::query("INSERT INTO note_search (note_id, title, body) VALUES (?, ?, ?)")
        .bind(note.note_id.to_string())
        .bind(&note.title)
        .bind(&note.body)
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;
    Ok(())
}

async fn insert_note_row(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    note: &Note,
) -> Result<(), SqliteStoreError> {
    let tags_json = serde_json::to_string(&note.tags).map_err(|_| SqliteStoreError::CorruptNote)?;
    sqlx::query(
        "INSERT INTO notes (note_id, creator_issuer, creator_subject, title, body, tags_json, created_at_ms, updated_at_ms, revision, deleted_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(note.note_id.to_string())
    .bind(&note.creator_issuer)
    .bind(&note.creator_subject)
    .bind(&note.title)
    .bind(&note.body)
    .bind(tags_json)
    .bind(note.created_at.get())
    .bind(note.updated_at.get())
    .bind(note.revision)
    .bind(note.deleted_at.map(UnixMillis::get))
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

fn note_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Note, SqliteStoreError> {
    let note_id = row
        .try_get::<String, _>("note_id")
        .map_err(database_error)?
        .parse::<EntityId>()
        .map(NoteId::new)
        .map_err(|_| SqliteStoreError::CorruptNote)?;
    let tags_json = row
        .try_get::<String, _>("tags_json")
        .map_err(database_error)?;
    let tags = serde_json::from_str(&tags_json).map_err(|_| SqliteStoreError::CorruptNote)?;
    Ok(Note {
        note_id,
        creator_issuer: row.try_get("creator_issuer").map_err(database_error)?,
        creator_subject: row.try_get("creator_subject").map_err(database_error)?,
        title: row.try_get("title").map_err(database_error)?,
        body: row.try_get("body").map_err(database_error)?,
        tags,
        created_at: UnixMillis::new(row.try_get("created_at_ms").map_err(database_error)?),
        updated_at: UnixMillis::new(row.try_get("updated_at_ms").map_err(database_error)?),
        revision: row.try_get("revision").map_err(database_error)?,
        deleted_at: row
            .try_get::<Option<i64>, _>("deleted_at_ms")
            .map_err(database_error)?
            .map(UnixMillis::new),
    })
}

fn session_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<AuthenticatedSession, SqliteStoreError> {
    Ok(AuthenticatedSession {
        actor: Actor {
            issuer: row.try_get("issuer").map_err(database_error)?,
            subject: row.try_get("subject").map_err(database_error)?,
            is_administrator: row
                .try_get::<bool, _>("is_administrator")
                .map_err(database_error)?,
        },
        idle_expires_at: UnixMillis::new(
            row.try_get("idle_expires_at_ms")
                .map_err(database_error)?,
        ),
        absolute_expires_at: UnixMillis::new(
            row.try_get("absolute_expires_at_ms")
                .map_err(database_error)?,
        ),
    })
}

fn database_error(error: sqlx::Error) -> SqliteStoreError {
    SqliteStoreError::Database(error.to_string())
}

fn permission_from_storage(value: i64) -> Result<NotePermission, SqliteStoreError> {
    match value {
        1 => Ok(NotePermission::Read),
        2 => Ok(NotePermission::Write),
        3 => Ok(NotePermission::Admin),
        _ => Err(SqliteStoreError::CorruptNote),
    }
}

fn permission_to_storage(value: NotePermission) -> i64 {
    match value {
        NotePermission::Read => 1,
        NotePermission::Write => 2,
        NotePermission::Admin => 3,
    }
}

impl OidcLoginAttemptStore for SqliteOidcLoginAttemptStore {
    type Error = sqlx::Error;

    fn issue(
        &self,
        attempt: OidcLoginAttempt,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO oidc_login_attempts (state_hash, nonce, pkce_verifier, expires_at_ms)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(hash_token(&attempt.state))
            .bind(attempt.nonce)
            .bind(attempt.pkce_verifier)
            .bind(attempt.expires_at.get())
            .execute(&pool)
            .await?;
            Ok(())
        }
    }

    fn consume(
        &self,
        state: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<OidcLoginAttempt>, Self::Error>> + Send {
        let pool = self.pool.clone();
        async move {
            let hash = hash_token(&state);
            let row = sqlx::query(
                "DELETE FROM oidc_login_attempts
                 WHERE state_hash = ? AND expires_at_ms > ?
                 RETURNING nonce, pkce_verifier, expires_at_ms",
            )
            .bind(&hash)
            .bind(now.get())
            .fetch_optional(&pool)
            .await?;
            sqlx::query("DELETE FROM oidc_login_attempts WHERE state_hash = ?")
                .bind(hash)
                .execute(&pool)
                .await?;
            row.map(|row| {
                Ok(OidcLoginAttempt {
                    state,
                    nonce: row.try_get("nonce")?,
                    pkce_verifier: row.try_get("pkce_verifier")?,
                    expires_at: UnixMillis::new(row.try_get("expires_at_ms")?),
                })
            })
            .transpose()
        }
    }
}
fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_application::McpRefreshTokenRotation;
    use marginalis_domain::{
        Actor, Archive, McpAuthorizationGrant, Note,
        NoteAclEntry, NoteDraft, WebSession, EntityId,
        McpOAuthClient, NoteId, NotePermission, UnixMillis,
    };

    use super::*;

    #[tokio::test]
    async fn single_source_updates_and_purges_notes_transactionally() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("v3 migration succeeds");
        let note_id = NoteId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("v7 note ID"),
        );
        let note = Note {
            note_id,
            creator_issuer: "https://id.example.test".into(),
            creator_subject: "alice".into(),
            title: "First title".into(),
            body: "first body".into(),
            tags: vec!["research".into()],
            created_at: UnixMillis::new(100),
            updated_at: UnixMillis::new(100),
            revision: 1,
            deleted_at: None,
        };
        database
            .create_note(&note, NotePermission::Admin)
            .await
            .expect("create note");
        assert_eq!(database.note(note_id, false).await, Ok(Some(note.clone())));
        assert_eq!(
            database.note_acl(note_id).await.expect("owner ACL"),
            vec![NoteAclEntry {
                issuer: "https://id.example.test".into(),
                subject: "alice".into(),
                permission: NotePermission::Admin,
            }]
        );
        assert_eq!(
            database
                .set_note_permission(
                    note_id,
                    "https://id.example.test",
                    "alice",
                    Some(NotePermission::Write),
                )
                .await,
            Err(SqliteStoreError::LastAdmin)
        );
        database
            .set_note_permission(
                note_id,
                "https://id.example.test",
                "bob",
                Some(NotePermission::Admin),
            )
            .await
            .expect("add second administrator");
        database
            .set_note_permission(
                note_id,
                "https://id.example.test",
                "alice",
                Some(NotePermission::Write),
            )
            .await
            .expect("downgrade after second administrator");
        let alice = Actor {
            issuer: "https://id.example.test".into(),
            subject: "alice".into(),
            is_administrator: false,
        };
        let charlie = Actor {
            issuer: "https://id.example.test".into(),
            subject: "charlie".into(),
            is_administrator: false,
        };
        let administrator = Actor {
            issuer: "https://id.example.test".into(),
            subject: "administrator".into(),
            is_administrator: true,
        };
        assert!(
            database
                .visible_note(&alice, note_id, NotePermission::Read)
                .await
                .expect("owner remains visible")
                .is_some()
        );
        assert_eq!(
            database
                .visible_note(&charlie, note_id, NotePermission::Read)
                .await,
            Ok(None)
        );
        assert_eq!(
            database
                .list_visible_notes(&administrator, 0, 10)
                .await
                .expect("administrator list")
                .len(),
            1
        );

        let updated = database
            .update_note(
                note_id,
                1,
                &NoteDraft {
                    title: "Updated title".into(),
                    body: "updated body".into(),
                    tags: vec!["research".into(), "v3".into()],
                },
                UnixMillis::new(200),
            )
            .await
            .expect("update note");
        assert_eq!(updated.revision, 2);
        assert_eq!(updated.title, "Updated title");
        assert_eq!(
            database
                .soft_delete_note(note_id, 1, UnixMillis::new(300))
                .await,
            Err(SqliteStoreError::Conflict)
        );
        database
            .soft_delete_note(note_id, 2, UnixMillis::new(300))
            .await
            .expect("soft delete");
        assert_eq!(database.note(note_id, false).await, Ok(None));
        let deleted = database
            .note(note_id, true)
            .await
            .expect("read deleted")
            .expect("deleted note remains");
        assert_eq!(deleted.deleted_at, Some(UnixMillis::new(300)));
        assert_eq!(deleted.revision, 3);

        let restored = database
            .restore_note(note_id, 3, UnixMillis::new(350))
            .await
            .expect("restore note");
        assert_eq!(restored.deleted_at, None);
        assert_eq!(restored.revision, 4);
        let archive = database.export_archive().await.expect("export archive");
        let imported_database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("empty import target");
        imported_database
            .import_archive(&archive)
            .await
            .expect("import archive");
        assert_eq!(
            imported_database
                .export_archive()
                .await
                .expect("re-export archive"),
            archive
        );
        assert_eq!(
            imported_database.import_archive(&archive).await,
            Err(SqliteStoreError::ArchiveTargetNotEmpty)
        );
        let mut invalid_archive = archive.clone();
        invalid_archive.notes[0].note.tags = vec![" duplicate ".into(), "duplicate".into()];
        let rejected_database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("empty rejected target");
        assert_eq!(
            rejected_database.import_archive(&invalid_archive).await,
            Err(SqliteStoreError::CorruptNote)
        );
        assert_eq!(
            rejected_database
                .export_archive()
                .await
                .expect("empty archive"),
            Archive {
                format: ARCHIVE_FORMAT.into(),
                notes: Vec::new(),
            }
        );
        database
            .soft_delete_note(note_id, 4, UnixMillis::new(400))
            .await
            .expect("delete before purge");
        assert_eq!(
            database
                .purge_deleted_before(UnixMillis::new(401))
                .await
                .expect("purge"),
            1
        );
        assert_eq!(database.note(note_id, true).await, Ok(None));
    }

    #[tokio::test]
    async fn sessions_retain_login_time_group_snapshot() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("v3 migration succeeds");
        let session = WebSession {
            session_id: "session-token".into(),
            csrf_token: "csrf-token".into(),
            actor: Actor {
                issuer: "https://id.example.test".into(),
                subject: "alice".into(),
                is_administrator: false,
            },
            idle_expires_at: UnixMillis::new(1_000),
            absolute_expires_at: UnixMillis::new(2_000),
        };
        database
            .issue_web_session(&session, UnixMillis::new(100))
            .await
            .expect("issue session");
        assert!(
            database
                .validate_web_session_csrf("session-token", "csrf-token")
                .await
                .expect("csrf query")
        );
        assert!(
            !database
                .validate_web_session_csrf("session-token", "wrong")
                .await
                .expect("csrf query")
        );
        let authenticated = database
            .lookup_web_session("session-token", UnixMillis::new(200))
            .await
            .expect("lookup")
            .expect("active session");
        assert!(!authenticated.actor.is_administrator);
    }

    #[tokio::test]
    async fn schema_contains_oauth_tables_bound_to_kanidm_subjects() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        for table in [
            "mcp_clients",
            "mcp_authorization_codes",
            "mcp_access_tokens",
            "mcp_refresh_tokens",
        ] {
            let exists = sqlx::query_scalar::<_, i64>(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?",
            )
            .bind(table)
            .fetch_optional(database.pool())
            .await
            .expect("schema query")
            .is_some();
            assert!(exists, "{table} must exist");
        }
        let client = McpOAuthClient {
            client_id: "https://client.example.test/mcp.json".into(),
            display_name: "Test client".into(),
            redirect_uris: vec!["https://client.example.test/callback".into()],
        };
        database
            .upsert_mcp_client(&client, UnixMillis::new(0))
            .await
            .expect("client");
        assert_eq!(
            database
                .mcp_client(&client.client_id)
                .await
                .expect("lookup"),
            Some(client.clone())
        );
        let grant = McpAuthorizationGrant {
            actor: Actor {
                issuer: "https://id.example.test".into(),
                subject: "alice".into(),
                is_administrator: false,
            },
            client_id: client.client_id.clone(),
            redirect_uri: client.redirect_uris[0].clone(),
            resource_uri: "https://notes.example.test/mcp".into(),
            scopes: vec!["notes:read".into()],
        };
        database
            .issue_mcp_authorization_code("code", &grant, "challenge", UnixMillis::new(100))
            .await
            .expect("code");
        assert!(
            database
                .consume_mcp_authorization_code(
                    "code",
                    &grant.client_id,
                    &grant.redirect_uri,
                    &grant.resource_uri,
                    "wrong-challenge",
                    UnixMillis::new(1)
                )
                .await
                .expect("wrong PKCE challenge")
                .is_none()
        );
        assert!(
            database
                .consume_mcp_authorization_code(
                    "code",
                    &grant.client_id,
                    &grant.redirect_uri,
                    &grant.resource_uri,
                    "challenge",
                    UnixMillis::new(1)
                )
                .await
                .expect("consume")
                .is_some()
        );
        assert!(
            database
                .consume_mcp_authorization_code(
                    "code",
                    &grant.client_id,
                    &grant.redirect_uri,
                    &grant.resource_uri,
                    "challenge",
                    UnixMillis::new(1)
                )
                .await
                .expect("second consume")
                .is_none()
        );
        database
            .issue_mcp_token_pair(
                "access",
                "refresh",
                &grant,
                UnixMillis::new(100),
                UnixMillis::new(1_000),
                UnixMillis::new(1),
            )
            .await
            .expect("token pair");
        assert!(
            database
                .authenticate_mcp_access_token(
                    "access",
                    &grant.resource_uri,
                    "notes:read",
                    UnixMillis::new(2)
                )
                .await
                .expect("access token")
                .is_some()
        );
        assert!(
            database
                .rotate_mcp_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "refresh".into(),
                        client_id: grant.client_id.clone(),
                        resource_uri: grant.resource_uri.clone(),
                        new_access_token: "next-access".into(),
                        new_refresh_token: "next-refresh".into(),
                        access_expires_at: UnixMillis::new(200),
                        refresh_expires_at: UnixMillis::new(2_000),
                    },
                    UnixMillis::new(3)
                )
                .await
                .expect("rotation")
                .is_some()
        );
        assert!(
            database
                .authenticate_mcp_access_token(
                    "next-access",
                    &grant.resource_uri,
                    "notes:read",
                    UnixMillis::new(4)
                )
                .await
                .expect("rotated access")
                .is_some()
        );
        assert!(
            database
                .register_mcp_client_bounded(
                    &McpOAuthClient {
                        client_id: "another-client".into(),
                        display_name: "Another client".into(),
                        redirect_uris: vec!["https://other.example.test/callback".into()],
                    },
                    UnixMillis::new(5),
                    UnixMillis::new(0),
                    10,
                )
                .await
                .expect("registration cleanup")
        );
        assert!(
            database
                .rotate_mcp_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "refresh".into(),
                        client_id: "different-client".into(),
                        resource_uri: grant.resource_uri.clone(),
                        new_access_token: "wrong-binding-access".into(),
                        new_refresh_token: "wrong-binding-refresh".into(),
                        access_expires_at: UnixMillis::new(200),
                        refresh_expires_at: UnixMillis::new(2_000),
                    },
                    UnixMillis::new(6)
                )
                .await
                .expect("wrong binding")
                .is_none()
        );
        assert!(
            database
                .authenticate_mcp_access_token(
                    "next-access",
                    &grant.resource_uri,
                    "notes:read",
                    UnixMillis::new(7)
                )
                .await
                .expect("access after wrong binding")
                .is_some()
        );
        assert!(
            database
                .rotate_mcp_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "refresh".into(),
                        client_id: grant.client_id.clone(),
                        resource_uri: grant.resource_uri.clone(),
                        new_access_token: "again-access".into(),
                        new_refresh_token: "again-refresh".into(),
                        access_expires_at: UnixMillis::new(200),
                        refresh_expires_at: UnixMillis::new(2_000),
                    },
                    UnixMillis::new(8)
                )
                .await
                .expect("refresh token replay")
                .is_none()
        );
        assert!(
            database
                .authenticate_mcp_access_token(
                    "next-access",
                    &grant.resource_uri,
                    "notes:read",
                    UnixMillis::new(9)
                )
                .await
                .expect("access after replay")
                .is_none()
        );
        assert!(
            database
                .rotate_mcp_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "next-refresh".into(),
                        client_id: grant.client_id.clone(),
                        resource_uri: grant.resource_uri.clone(),
                        new_access_token: "post-replay-access".into(),
                        new_refresh_token: "post-replay-refresh".into(),
                        access_expires_at: UnixMillis::new(200),
                        refresh_expires_at: UnixMillis::new(2_000),
                    },
                    UnixMillis::new(10)
                )
                .await
                .expect("family after replay")
                .is_none()
        );
    }

    #[tokio::test]
    async fn dynamic_registration_prunes_stale_unreferenced_clients() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        database
            .upsert_mcp_client(
                &McpOAuthClient {
                    client_id: "stale-client".into(),
                    display_name: "Stale client".into(),
                    redirect_uris: vec!["https://client.example.test/callback".into()],
                },
                UnixMillis::new(0),
            )
            .await
            .expect("client");
        assert!(
            database
                .register_mcp_client_bounded(
                    &McpOAuthClient {
                        client_id: "fresh-client".into(),
                        display_name: "Fresh client".into(),
                        redirect_uris: vec!["https://client.example.test/callback".into()],
                    },
                    UnixMillis::new(2 * 24 * 60 * 60 * 1_000),
                    UnixMillis::new(24 * 60 * 60 * 1_000),
                    1,
                )
                .await
                .expect("register")
        );
        assert!(
            database
                .mcp_client("stale-client")
                .await
                .expect("lookup")
                .is_none()
        );
        assert!(
            database
                .mcp_client("fresh-client")
                .await
                .expect("lookup")
                .is_some()
        );
    }


}

