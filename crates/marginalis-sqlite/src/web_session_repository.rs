//! applicationのWeb session repository portに対するSQLite実装。

use async_trait::async_trait;
use marginalis_application::{SessionRepositoryError, WebSessionRepository};
use marginalis_domain::{AuthenticatedSession, UnixMillis, WebSession};

use crate::SqliteDatabase;

#[async_trait]
impl WebSessionRepository for SqliteDatabase {
    async fn lookup(
        &self,
        session_id: &str,
        now: UnixMillis,
        idle_timeout_ms: i64,
    ) -> Result<Option<AuthenticatedSession>, SessionRepositoryError> {
        self.lookup_web_session(session_id, now, idle_timeout_ms)
            .await
            .map_err(|_| SessionRepositoryError)
    }

    async fn verify_csrf(
        &self,
        session_id: &str,
        csrf_token: &str,
    ) -> Result<bool, SessionRepositoryError> {
        self.validate_web_session_csrf(session_id, csrf_token)
            .await
            .map_err(|_| SessionRepositoryError)
    }

    async fn issue(
        &self,
        session: &WebSession,
        now: UnixMillis,
    ) -> Result<(), SessionRepositoryError> {
        self.issue_web_session(session, now)
            .await
            .map_err(|_| SessionRepositoryError)
    }

    async fn revoke(
        &self,
        session_id: &str,
        now: UnixMillis,
    ) -> Result<(), SessionRepositoryError> {
        self.revoke_web_session(session_id, now)
            .await
            .map_err(|_| SessionRepositoryError)
    }
}
