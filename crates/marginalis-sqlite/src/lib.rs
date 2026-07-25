//! Marginalisの現行データモデルに限定したSQLite adapter。

mod archive;
mod mcp;
mod notes;
mod schema;
mod session;
mod token;

pub use session::SqliteOidcLoginAttemptStore;

use crate::schema::migrate;
use std::{fmt, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

#[derive(Clone, Debug)]
pub struct SqliteDatabase {
    pub(crate) pool: SqlitePool,
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
}

pub(crate) fn database_error(error: sqlx::Error) -> SqliteStoreError {
    SqliteStoreError::Database(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_application::{
        McpRefreshTokenRotation, OidcLoginAttempt, OidcLoginAttemptStore,
    };
    use marginalis_domain::{
        ARCHIVE_FORMAT, Actor, Archive, EntityId, McpAuthorizationGrant, McpOAuthClient, Note,
        NoteAclEntry, NoteDraft, NoteId, NotePermission, SOFT_DELETE_RETENTION_MS, UnixMillis,
        WebSession,
    };

    use super::*;

    #[tokio::test]
    async fn initialization_rejects_a_database_with_unknown_tables() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("database");
        sqlx::query("CREATE TABLE unknown_notes (note_id TEXT PRIMARY KEY NOT NULL) STRICT")
            .execute(&pool)
            .await
            .expect("unknown table");

        let error = migrate(&pool)
            .await
            .expect_err("non-empty database must be rejected");
        assert!(
            error
                .to_string()
                .contains("initialization requires an empty database")
        );
        assert!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'notes'"
            )
            .fetch_one(&pool)
            .await
            .expect("schema query")
                == 0
        );
    }

    #[tokio::test]
    async fn single_source_updates_and_purges_notes_transactionally() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("schema initialization succeeds");
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
        invalid_archive.notes[0].note.creator_issuer.clear();
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
                .restore_note(
                    note_id,
                    5,
                    UnixMillis::new(400 + SOFT_DELETE_RETENTION_MS + 1)
                )
                .await,
            Err(SqliteStoreError::Conflict)
        );
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
            .expect("schema initialization succeeds");
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
            .lookup_web_session("session-token", UnixMillis::new(200), 900)
            .await
            .expect("lookup")
            .expect("active session");
        assert!(!authenticated.actor.is_administrator);
        assert_eq!(authenticated.idle_expires_at, UnixMillis::new(1_100));
        assert_eq!(
            database
                .lookup_web_session("session-token", UnixMillis::new(1_050), 900)
                .await
                .expect("sliding lookup")
                .expect("activity extends the session")
                .idle_expires_at,
            UnixMillis::new(1_950)
        );
        assert_eq!(
            database
                .lookup_web_session("session-token", UnixMillis::new(1_900), 900)
                .await
                .expect("absolute cap lookup")
                .expect("session remains active before the absolute limit")
                .idle_expires_at,
            UnixMillis::new(2_000)
        );
        assert_eq!(
            database
                .lookup_web_session("session-token", UnixMillis::new(2_000), 900)
                .await,
            Ok(None)
        );
        let replacement = WebSession {
            session_id: "replacement-session".into(),
            csrf_token: "replacement-csrf".into(),
            actor: session.actor,
            idle_expires_at: UnixMillis::new(3_000),
            absolute_expires_at: UnixMillis::new(4_000),
        };
        database
            .issue_web_session(&replacement, UnixMillis::new(2_100))
            .await
            .expect("issuing a session cleans expired and revoked rows");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM web_sessions")
                .fetch_one(database.pool())
                .await
                .expect("session count"),
            1
        );
    }

    #[tokio::test]
    async fn oidc_attempt_issue_removes_expired_rows() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("schema initialization succeeds");
        let attempts = database.oidc_login_attempt_store();
        attempts
            .issue(
                OidcLoginAttempt {
                    state: "expired-state".into(),
                    nonce: "expired-nonce".into(),
                    pkce_verifier: "expired-verifier".into(),
                    expires_at: UnixMillis::new(1_000),
                },
                UnixMillis::new(100),
            )
            .await
            .expect("first attempt");
        attempts
            .issue(
                OidcLoginAttempt {
                    state: "active-state".into(),
                    nonce: "active-nonce".into(),
                    pkce_verifier: "active-verifier".into(),
                    expires_at: UnixMillis::new(2_000),
                },
                UnixMillis::new(1_000),
            )
            .await
            .expect("second attempt cleans the expired row");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM oidc_login_attempts")
                .fetch_one(database.pool())
                .await
                .expect("attempt count"),
            1
        );
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
