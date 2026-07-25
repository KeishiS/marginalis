//! OIDC login時の権限snapshotを保持するCookie session use case。

use async_trait::async_trait;
use marginalis_application::{
    AuthenticationUseCaseError, Clock, Random, SessionLifetime, WebSessionUseCases,
};
use marginalis_domain::{Actor, AuthenticatedSession, UnixMillis, WebSession};
use marginalis_sqlite::SqliteDatabase;

use crate::{SystemClock, SystemRandom};

#[derive(Clone)]
pub struct ServerWebSessionUseCases {
    database: SqliteDatabase,
    lifetime: SessionLifetime,
}

impl ServerWebSessionUseCases {
    pub fn new(database: SqliteDatabase, lifetime: SessionLifetime) -> Self {
        Self { database, lifetime }
    }
}

#[async_trait]
impl WebSessionUseCases for ServerWebSessionUseCases {
    async fn authenticate_session(
        &self,
        session_id: String,
    ) -> Result<Option<AuthenticatedSession>, AuthenticationUseCaseError> {
        let now = SystemClock.now();
        let Some(session) = self
            .database
            .lookup_web_session(&session_id, now)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?
        else {
            return Ok(None);
        };
        Ok(Some(session))
    }

    async fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
    ) -> Result<bool, AuthenticationUseCaseError> {
        self.database
            .validate_web_session_csrf(&session_id, &csrf_token)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn issue_session(&self, actor: Actor) -> Result<WebSession, AuthenticationUseCaseError> {
        let now = SystemClock.now();
        let session = WebSession {
            session_id: SystemRandom.opaque_token(),
            csrf_token: SystemRandom.opaque_token(),
            actor,
            idle_expires_at: UnixMillis::new(now.get() + self.lifetime.idle_timeout_ms),
            absolute_expires_at: UnixMillis::new(now.get() + self.lifetime.absolute_timeout_ms),
        };
        self.database
            .issue_web_session(&session, now)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        Ok(session)
    }

    async fn revoke_session(&self, session_id: String) -> Result<(), AuthenticationUseCaseError> {
        self.database
            .revoke_web_session(&session_id, SystemClock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }
}
