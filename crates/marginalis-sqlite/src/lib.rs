//! Marginalisの現行データモデルに限定したSQLite adapter。

mod archive;
mod cleanup;
mod diagnostics;
mod mcp;
mod notes;
mod schema;
mod session;
mod token;

pub use cleanup::AuthStatePurgeCounts;
pub use diagnostics::SqliteDiagnosticReport;
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
    NotFound,
    Conflict,
    ArchiveTargetNotEmpty,
    CorruptData,
    Database(String),
}

impl fmt::Display for SqliteStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("note was not found or is not accessible"),
            Self::Conflict => formatter.write_str("note revision does not match"),
            Self::ArchiveTargetNotEmpty => {
                formatter.write_str("archive import target must be empty")
            }
            Self::CorruptData => formatter.write_str("stored data is invalid"),
            Self::Database(_) => formatter.write_str("database query failed"),
        }
    }
}

impl std::error::Error for SqliteStoreError {}

impl SqliteDatabase {
    /// 現行のSQLite schemaへ接続する。
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

    /// databaseを変更せず、利用可否、schema、整合性を検査する。
    pub async fn diagnose(database_url: &str) -> SqliteDiagnosticReport {
        diagnostics::diagnose(database_url).await
    }
}

pub(crate) fn database_error(error: sqlx::Error) -> SqliteStoreError {
    SqliteStoreError::Database(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_application::{
        McpAuthorizationCodeExchange, McpRefreshTokenRotation, McpRefreshTokenRotationOutcome,
        OidcLoginAttempt, OidcLoginAttemptStore,
    };
    use marginalis_domain::{
        Actor, EntityId, McpAuthorizationGrant, McpOAuthClient, Note, NoteDraft, NoteId,
        SOFT_DELETE_RETENTION_MS, UnixMillis, WebSession,
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
    async fn initialization_rejects_the_previous_schema_version() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("database");
        sqlx::query("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY NOT NULL) STRICT")
            .execute(&pool)
            .await
            .expect("migration table");
        sqlx::query("INSERT INTO schema_migrations (version) VALUES (3)")
            .execute(&pool)
            .await
            .expect("old version");

        let error = migrate(&pool)
            .await
            .expect_err("old schema must be rejected");
        assert!(
            error
                .to_string()
                .contains("unsupported database schema version 3; expected 4")
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
        database.create_note(&note).await.expect("create note");
        assert_eq!(database.note(note_id, false).await, Ok(Some(note.clone())));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'note_acl'"
            )
            .fetch_one(&database.pool)
            .await
            .expect("schema query"),
            0
        );
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
        let same_subject_different_issuer = Actor {
            issuer: "https://other-id.example.test".into(),
            subject: "alice".into(),
            is_administrator: false,
        };
        let administrator = Actor {
            issuer: "https://id.example.test".into(),
            subject: "administrator".into(),
            is_administrator: true,
        };
        assert!(
            database
                .visible_note(&alice, note_id)
                .await
                .expect("owner is visible")
                .is_some()
        );
        assert_eq!(database.visible_note(&charlie, note_id).await, Ok(None));
        assert_eq!(
            database
                .visible_note(&same_subject_different_issuer, note_id)
                .await,
            Ok(None)
        );
        assert_eq!(
            database
                .list_visible_notes(&alice)
                .await
                .expect("owner list")
                .len(),
            1
        );
        assert!(
            database
                .list_visible_notes(&charlie)
                .await
                .expect("non-owner list")
                .is_empty()
        );
        assert!(
            database
                .list_visible_notes(&same_subject_different_issuer)
                .await
                .expect("different issuer list")
                .is_empty()
        );
        assert_eq!(
            database
                .list_visible_notes(&administrator)
                .await
                .expect("administrator list")
                .len(),
            1
        );
        assert_eq!(
            database
                .update_visible_note(
                    &charlie,
                    note_id,
                    1,
                    &NoteDraft {
                        title: "Unauthorized title".into(),
                        body: "must not persist".into(),
                        tags: vec![],
                    },
                    UnixMillis::new(150),
                )
                .await,
            Err(SqliteStoreError::NotFound)
        );
        assert_eq!(
            database
                .note(note_id, false)
                .await
                .expect("note query")
                .expect("note remains")
                .title,
            "First title"
        );

        let updated = database
            .update_visible_note(
                &alice,
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
                .soft_delete_visible_note(&administrator, note_id, 1, UnixMillis::new(300))
                .await,
            Err(SqliteStoreError::Conflict)
        );
        let deleted = database
            .soft_delete_visible_note(&administrator, note_id, 2, UnixMillis::new(300))
            .await
            .expect("soft delete");
        assert_eq!(deleted.deleted_at, Some(UnixMillis::new(300)));
        assert_eq!(database.note(note_id, false).await, Ok(None));
        let deleted = database
            .note(note_id, true)
            .await
            .expect("read deleted")
            .expect("deleted note remains");
        assert_eq!(deleted.deleted_at, Some(UnixMillis::new(300)));
        assert_eq!(deleted.revision, 3);

        let restored = database
            .restore_visible_note(&administrator, note_id, 3, UnixMillis::new(350))
            .await
            .expect("restore note");
        assert_eq!(restored.deleted_at, None);
        assert_eq!(restored.revision, 4);
        let snapshot = database.export_notes().await.expect("export snapshot");
        let imported_database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("empty import target");
        imported_database
            .import_notes(&snapshot)
            .await
            .expect("import snapshot");
        assert_eq!(
            imported_database
                .export_notes()
                .await
                .expect("re-export snapshot"),
            snapshot
        );
        assert_eq!(
            imported_database.import_notes(&snapshot).await,
            Err(SqliteStoreError::ArchiveTargetNotEmpty)
        );
        let nonempty_auth_database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("auth-state import target");
        nonempty_auth_database
            .oidc_login_attempt_store()
            .issue(
                OidcLoginAttempt {
                    state: "pending-state".into(),
                    nonce: "nonce".into(),
                    pkce_verifier: "verifier".into(),
                    expires_at: UnixMillis::new(1_000),
                },
                UnixMillis::new(0),
            )
            .await
            .expect("pending login attempt");
        assert_eq!(
            nonempty_auth_database.import_notes(&snapshot).await,
            Err(SqliteStoreError::ArchiveTargetNotEmpty)
        );
        let mut invalid_snapshot = snapshot.clone();
        invalid_snapshot[0].creator_issuer.clear();
        let rejected_database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("empty rejected target");
        assert_eq!(
            rejected_database.import_notes(&invalid_snapshot).await,
            Err(SqliteStoreError::CorruptData)
        );
        let mut injected_identity = snapshot.clone();
        injected_identity[0].creator_subject = "alice\n:admin: true".into();
        assert_eq!(
            rejected_database.import_notes(&injected_identity).await,
            Err(SqliteStoreError::CorruptData)
        );
        let mut invalid_deleted_at = snapshot.clone();
        invalid_deleted_at[0].deleted_at =
            Some(UnixMillis::new(invalid_deleted_at[0].updated_at.get() + 1));
        assert_eq!(
            rejected_database.import_notes(&invalid_deleted_at).await,
            Err(SqliteStoreError::CorruptData)
        );
        assert_eq!(
            rejected_database
                .export_notes()
                .await
                .expect("empty snapshot"),
            Vec::new()
        );
        database
            .soft_delete_visible_note(&administrator, note_id, 4, UnixMillis::new(400))
            .await
            .expect("delete before purge");
        assert_eq!(
            database
                .restore_visible_note(
                    &administrator,
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
            .expect("issue replacement session");
        let counts = database
            .purge_expired_auth_state(UnixMillis::new(2_100), UnixMillis::new(0))
            .await
            .expect("explicit session cleanup");
        assert_eq!(counts.web_sessions, 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM web_sessions")
                .fetch_one(&database.pool)
                .await
                .expect("session count"),
            1
        );
    }

    #[tokio::test]
    async fn explicit_auth_cleanup_removes_expired_rows_without_new_issuance() {
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
                UnixMillis::new(100),
            )
            .await
            .expect("active attempt");
        attempts
            .issue(
                OidcLoginAttempt {
                    state: "consumed-expired-state".into(),
                    nonce: "expired-nonce".into(),
                    pkce_verifier: "expired-verifier".into(),
                    expires_at: UnixMillis::new(1_000),
                },
                UnixMillis::new(100),
            )
            .await
            .expect("expired attempt to consume");
        assert_eq!(
            attempts
                .consume("consumed-expired-state".into(), UnixMillis::new(1_000))
                .await
                .expect("consume expired attempt"),
            None
        );
        assert_eq!(
            attempts
                .consume("consumed-expired-state".into(), UnixMillis::new(1_000))
                .await
                .expect("replay consumed attempt"),
            None
        );
        let counts = database
            .purge_expired_auth_state(UnixMillis::new(1_000), UnixMillis::new(0))
            .await
            .expect("explicit cleanup");
        assert_eq!(counts.oidc_login_attempts, 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM oidc_login_attempts")
                .fetch_one(&database.pool)
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
            .fetch_optional(&database.pool)
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
            scopes: vec!["notes:read".into(), "notes:write".into()],
        };
        database
            .issue_mcp_authorization_code("code", &grant, "challenge", UnixMillis::new(100))
            .await
            .expect("code");
        assert!(
            database
                .exchange_mcp_authorization_code(
                    McpAuthorizationCodeExchange {
                        code: "code".into(),
                        client_id: grant.client_id.clone(),
                        redirect_uri: Some(grant.redirect_uri.clone()),
                        resource_uri: grant.resource_uri.clone(),
                        code_challenge: "wrong-challenge".into(),
                        access_token: "wrong-access".into(),
                        refresh_token: "wrong-refresh".into(),
                        access_expires_at: UnixMillis::new(100),
                        refresh_expires_at: UnixMillis::new(1_000),
                    },
                    UnixMillis::new(1),
                )
                .await
                .expect("wrong PKCE challenge")
                .is_none()
        );
        assert!(
            database
                .exchange_mcp_authorization_code(
                    McpAuthorizationCodeExchange {
                        code: "code".into(),
                        client_id: grant.client_id.clone(),
                        redirect_uri: Some(grant.redirect_uri.clone()),
                        resource_uri: grant.resource_uri.clone(),
                        code_challenge: "challenge".into(),
                        access_token: "access".into(),
                        refresh_token: "refresh".into(),
                        access_expires_at: UnixMillis::new(100),
                        refresh_expires_at: UnixMillis::new(1_000),
                    },
                    UnixMillis::new(1),
                )
                .await
                .expect("exchange")
                .is_some()
        );
        assert!(
            database
                .authenticate_mcp_access_token("access", &grant.resource_uri, UnixMillis::new(2))
                .await
                .expect("access token")
                .is_some()
        );
        assert!(matches!(
            database
                .rotate_mcp_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "refresh".into(),
                        client_id: grant.client_id.clone(),
                        resource_uri: grant.resource_uri.clone(),
                        requested_scopes: Some(vec!["notes:delete".into()]),
                        new_access_token: "escalated-access".into(),
                        new_refresh_token: "escalated-refresh".into(),
                        access_expires_at: UnixMillis::new(200),
                        refresh_expires_at: UnixMillis::new(2_000),
                    },
                    UnixMillis::new(2)
                )
                .await
                .expect("scope escalation"),
            McpRefreshTokenRotationOutcome::InvalidScope
        ));
        assert!(matches!(
            database
                .rotate_mcp_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "refresh".into(),
                        client_id: grant.client_id.clone(),
                        resource_uri: grant.resource_uri.clone(),
                        requested_scopes: Some(vec!["notes:read".into()]),
                        new_access_token: "next-access".into(),
                        new_refresh_token: "next-refresh".into(),
                        access_expires_at: UnixMillis::new(200),
                        refresh_expires_at: UnixMillis::new(2_000),
                    },
                    UnixMillis::new(3)
                )
                .await
                .expect("rotation"),
            McpRefreshTokenRotationOutcome::Rotated { .. }
        ));
        let rotated_actor = database
            .authenticate_mcp_access_token("next-access", &grant.resource_uri, UnixMillis::new(4))
            .await
            .expect("rotated access")
            .expect("authenticated actor");
        assert_eq!(rotated_actor.scopes, vec!["notes:read"]);
        assert!(
            database
                .register_mcp_client_bounded(
                    &McpOAuthClient {
                        client_id: "another-client".into(),
                        display_name: "Another client".into(),
                        redirect_uris: vec!["https://other.example.test/callback".into()],
                    },
                    UnixMillis::new(5),
                    10,
                )
                .await
                .expect("registration")
        );
        assert!(matches!(
            database
                .rotate_mcp_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "refresh".into(),
                        client_id: "different-client".into(),
                        resource_uri: grant.resource_uri.clone(),
                        requested_scopes: None,
                        new_access_token: "wrong-binding-access".into(),
                        new_refresh_token: "wrong-binding-refresh".into(),
                        access_expires_at: UnixMillis::new(200),
                        refresh_expires_at: UnixMillis::new(2_000),
                    },
                    UnixMillis::new(6)
                )
                .await
                .expect("wrong binding"),
            McpRefreshTokenRotationOutcome::InvalidToken
        ));
        assert!(
            database
                .authenticate_mcp_access_token(
                    "next-access",
                    &grant.resource_uri,
                    UnixMillis::new(7)
                )
                .await
                .expect("access after wrong binding")
                .is_some()
        );
        assert!(matches!(
            database
                .rotate_mcp_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "refresh".into(),
                        client_id: grant.client_id.clone(),
                        resource_uri: grant.resource_uri.clone(),
                        requested_scopes: None,
                        new_access_token: "again-access".into(),
                        new_refresh_token: "again-refresh".into(),
                        access_expires_at: UnixMillis::new(200),
                        refresh_expires_at: UnixMillis::new(2_000),
                    },
                    UnixMillis::new(8)
                )
                .await
                .expect("refresh token replay"),
            McpRefreshTokenRotationOutcome::InvalidToken
        ));
        assert!(
            database
                .authenticate_mcp_access_token(
                    "next-access",
                    &grant.resource_uri,
                    UnixMillis::new(9)
                )
                .await
                .expect("access after replay")
                .is_none()
        );
        assert!(matches!(
            database
                .rotate_mcp_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "next-refresh".into(),
                        client_id: grant.client_id.clone(),
                        resource_uri: grant.resource_uri.clone(),
                        requested_scopes: None,
                        new_access_token: "post-replay-access".into(),
                        new_refresh_token: "post-replay-refresh".into(),
                        access_expires_at: UnixMillis::new(200),
                        refresh_expires_at: UnixMillis::new(2_000),
                    },
                    UnixMillis::new(10)
                )
                .await
                .expect("family after replay"),
            McpRefreshTokenRotationOutcome::InvalidToken
        ));
    }

    #[tokio::test]
    async fn explicit_auth_cleanup_prunes_stale_unreferenced_clients() {
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
        let now = UnixMillis::new(2 * 24 * 60 * 60 * 1_000);
        let counts = database
            .purge_expired_auth_state(now, UnixMillis::new(24 * 60 * 60 * 1_000))
            .await
            .expect("cleanup");
        assert_eq!(counts.mcp_clients, 1);
        assert!(
            database
                .register_mcp_client_bounded(
                    &McpOAuthClient {
                        client_id: "fresh-client".into(),
                        display_name: "Fresh client".into(),
                        redirect_uris: vec!["https://client.example.test/callback".into()],
                    },
                    now,
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

    #[tokio::test]
    async fn authorization_code_replay_revokes_the_issued_token_family() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let client = McpOAuthClient {
            client_id: "client".into(),
            display_name: "Client".into(),
            redirect_uris: vec!["https://client.example/callback".into()],
        };
        database
            .upsert_mcp_client(&client, UnixMillis::new(0))
            .await
            .expect("client");
        let grant = McpAuthorizationGrant {
            actor: Actor {
                issuer: "https://id.example".into(),
                subject: "alice".into(),
                is_administrator: false,
            },
            client_id: client.client_id.clone(),
            redirect_uri: client.redirect_uris[0].clone(),
            resource_uri: "https://notes.example/mcp".into(),
            scopes: vec!["notes:read".into()],
        };
        database
            .issue_mcp_authorization_code("code", &grant, "challenge", UnixMillis::new(100))
            .await
            .expect("authorization code");
        let exchanged = database
            .exchange_mcp_authorization_code(
                McpAuthorizationCodeExchange {
                    code: "code".into(),
                    client_id: grant.client_id.clone(),
                    redirect_uri: None,
                    resource_uri: grant.resource_uri.clone(),
                    code_challenge: "challenge".into(),
                    access_token: "access".into(),
                    refresh_token: "refresh".into(),
                    access_expires_at: UnixMillis::new(500),
                    refresh_expires_at: UnixMillis::new(900),
                },
                UnixMillis::new(1),
            )
            .await
            .expect("first exchange");
        assert!(exchanged.is_some());
        assert!(
            database
                .authenticate_mcp_access_token("access", &grant.resource_uri, UnixMillis::new(2))
                .await
                .expect("access token")
                .is_some()
        );
        assert!(
            database
                .register_mcp_client_bounded(
                    &McpOAuthClient {
                        client_id: "cleanup-trigger".into(),
                        display_name: "Cleanup trigger".into(),
                        redirect_uris: vec!["https://other.example/callback".into()],
                    },
                    UnixMillis::new(200),
                    10,
                )
                .await
                .expect("cleanup while token family is active")
        );

        let replay = database
            .exchange_mcp_authorization_code(
                McpAuthorizationCodeExchange {
                    code: "code".into(),
                    client_id: grant.client_id.clone(),
                    redirect_uri: None,
                    resource_uri: grant.resource_uri.clone(),
                    code_challenge: "challenge".into(),
                    access_token: "attacker-access".into(),
                    refresh_token: "attacker-refresh".into(),
                    access_expires_at: UnixMillis::new(500),
                    refresh_expires_at: UnixMillis::new(900),
                },
                UnixMillis::new(201),
            )
            .await
            .expect("replayed exchange");
        assert!(replay.is_none());
        assert!(
            database
                .authenticate_mcp_access_token("access", &grant.resource_uri, UnixMillis::new(202))
                .await
                .expect("revoked access token")
                .is_none()
        );
        assert!(matches!(
            database
                .rotate_mcp_refresh_token(
                    McpRefreshTokenRotation {
                        refresh_token: "refresh".into(),
                        client_id: grant.client_id,
                        resource_uri: grant.resource_uri,
                        requested_scopes: None,
                        new_access_token: "next-access".into(),
                        new_refresh_token: "next-refresh".into(),
                        access_expires_at: UnixMillis::new(500),
                        refresh_expires_at: UnixMillis::new(900),
                    },
                    UnixMillis::new(203),
                )
                .await
                .expect("revoked refresh token"),
            McpRefreshTokenRotationOutcome::InvalidToken
        ));
    }

    #[tokio::test]
    async fn token_issuance_failure_rolls_back_authorization_code_consumption() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let client = McpOAuthClient {
            client_id: "client".into(),
            display_name: "Client".into(),
            redirect_uris: vec!["https://client.example/callback".into()],
        };
        database
            .upsert_mcp_client(&client, UnixMillis::new(0))
            .await
            .expect("client");
        let grant = McpAuthorizationGrant {
            actor: Actor {
                issuer: "https://id.example".into(),
                subject: "alice".into(),
                is_administrator: false,
            },
            client_id: client.client_id.clone(),
            redirect_uri: client.redirect_uris[0].clone(),
            resource_uri: "https://notes.example/mcp".into(),
            scopes: vec!["notes:read".into()],
        };
        for code in ["first-code", "retryable-code"] {
            database
                .issue_mcp_authorization_code(code, &grant, "challenge", UnixMillis::new(1_000))
                .await
                .expect("authorization code");
        }
        database
            .exchange_mcp_authorization_code(
                McpAuthorizationCodeExchange {
                    code: "first-code".into(),
                    client_id: grant.client_id.clone(),
                    redirect_uri: None,
                    resource_uri: grant.resource_uri.clone(),
                    code_challenge: "challenge".into(),
                    access_token: "colliding-access".into(),
                    refresh_token: "first-refresh".into(),
                    access_expires_at: UnixMillis::new(500),
                    refresh_expires_at: UnixMillis::new(900),
                },
                UnixMillis::new(1),
            )
            .await
            .expect("first exchange")
            .expect("first grant");

        let failed = database
            .exchange_mcp_authorization_code(
                McpAuthorizationCodeExchange {
                    code: "retryable-code".into(),
                    client_id: grant.client_id.clone(),
                    redirect_uri: None,
                    resource_uri: grant.resource_uri.clone(),
                    code_challenge: "challenge".into(),
                    access_token: "colliding-access".into(),
                    refresh_token: "failed-refresh".into(),
                    access_expires_at: UnixMillis::new(500),
                    refresh_expires_at: UnixMillis::new(900),
                },
                UnixMillis::new(2),
            )
            .await;
        assert!(matches!(failed, Err(SqliteStoreError::Database(_))));

        let retried = database
            .exchange_mcp_authorization_code(
                McpAuthorizationCodeExchange {
                    code: "retryable-code".into(),
                    client_id: grant.client_id,
                    redirect_uri: None,
                    resource_uri: grant.resource_uri,
                    code_challenge: "challenge".into(),
                    access_token: "retry-access".into(),
                    refresh_token: "retry-refresh".into(),
                    access_expires_at: UnixMillis::new(500),
                    refresh_expires_at: UnixMillis::new(900),
                },
                UnixMillis::new(3),
            )
            .await
            .expect("retry after rollback");
        assert!(retried.is_some());
    }
}
