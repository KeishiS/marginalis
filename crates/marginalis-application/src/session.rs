//! Web sessionの業務処理と永続化port。

use std::sync::Arc;

use async_trait::async_trait;
use marginalis_domain::{Actor, AuthenticatedSession, UnixMillis, WebSession};

use crate::{AuthenticationUseCaseError, Clock, Random, SessionLifetime, WebSessionUseCases};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionRepositoryError;

/// Cookie sessionを保存する外向きport。
#[async_trait]
pub trait WebSessionRepository: Send + Sync {
    async fn lookup(
        &self,
        session_id: &str,
        now: UnixMillis,
        idle_timeout_ms: i64,
    ) -> Result<Option<AuthenticatedSession>, SessionRepositoryError>;
    async fn verify_csrf(
        &self,
        session_id: &str,
        csrf_token: &str,
    ) -> Result<bool, SessionRepositoryError>;
    async fn issue(
        &self,
        session: &WebSession,
        now: UnixMillis,
    ) -> Result<(), SessionRepositoryError>;
    async fn revoke(&self, session_id: &str, now: UnixMillis)
    -> Result<(), SessionRepositoryError>;
}

pub struct WebSessionApplication {
    repository: Arc<dyn WebSessionRepository>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
    lifetime: SessionLifetime,
}

impl WebSessionApplication {
    pub fn new(
        repository: Arc<dyn WebSessionRepository>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
        lifetime: SessionLifetime,
    ) -> Self {
        Self {
            repository,
            clock,
            random,
            lifetime,
        }
    }
}

#[async_trait]
impl WebSessionUseCases for WebSessionApplication {
    async fn authenticate_session(
        &self,
        session_id: String,
    ) -> Result<Option<AuthenticatedSession>, AuthenticationUseCaseError> {
        self.repository
            .lookup(&session_id, self.clock.now(), self.lifetime.idle_timeout_ms)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
    ) -> Result<bool, AuthenticationUseCaseError> {
        self.repository
            .verify_csrf(&session_id, &csrf_token)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn issue_session(&self, actor: Actor) -> Result<WebSession, AuthenticationUseCaseError> {
        let now = self.clock.now();
        let session = WebSession {
            session_id: self.random.opaque_token(),
            csrf_token: self.random.opaque_token(),
            actor,
            idle_expires_at: UnixMillis::new(now.get() + self.lifetime.idle_timeout_ms),
            absolute_expires_at: UnixMillis::new(now.get() + self.lifetime.absolute_timeout_ms),
        };
        self.repository
            .issue(&session, now)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)?;
        Ok(session)
    }

    async fn revoke_session(&self, session_id: String) -> Result<(), AuthenticationUseCaseError> {
        self.repository
            .revoke(&session_id, self.clock.now())
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }
}
