//! Kanidm OIDC loginと署名検証済みgroup claimをActorへ変換するuse case。

use std::sync::Arc;

use async_trait::async_trait;
use marginalis_application::{AuthenticationUseCaseError, OidcAuthenticationUseCases};
use marginalis_auth_oidc::{OidcAuthentication, OidcCallbackError, OidcConfiguration};
use marginalis_domain::Actor;
use marginalis_sqlite::SqliteDatabase;

use crate::{SystemClock, SystemRandom};

#[derive(Clone)]
pub struct ServerOidcAuthenticationUseCases {
    database: SqliteDatabase,
    configuration: OidcConfiguration,
    http_client: reqwest::Client,
    oidc: Arc<tokio::sync::RwLock<Option<OidcAuthentication>>>,
}

impl ServerOidcAuthenticationUseCases {
    pub fn new(
        database: SqliteDatabase,
        configuration: OidcConfiguration,
        http_client: reqwest::Client,
        oidc: Option<OidcAuthentication>,
    ) -> Self {
        Self {
            database,
            configuration,
            http_client,
            oidc: Arc::new(tokio::sync::RwLock::new(oidc)),
        }
    }

    /// Discovery失敗後も次のログイン要求で同じTLS設定を使って再試行する。
    async fn oidc(&self) -> Result<OidcAuthentication, AuthenticationUseCaseError> {
        if let Some(oidc) = self.oidc.read().await.clone() {
            return Ok(oidc);
        }
        let discovered = OidcAuthentication::discover_with_http_client(
            &self.configuration,
            self.http_client.clone(),
        )
        .await
        .map_err(|_| {
            tracing::warn!(
                event = "oidc.discovery.failed",
                error_kind = "unavailable",
                "OIDC discovery retry failed"
            );
            AuthenticationUseCaseError::Unavailable
        })?;
        tracing::info!(
            event = "oidc.discovery.completed",
            "OIDC discovery succeeded"
        );
        let mut oidc = self.oidc.write().await;
        Ok(oidc.get_or_insert(discovered).clone())
    }
}

#[async_trait]
impl OidcAuthenticationUseCases for ServerOidcAuthenticationUseCases {
    async fn begin_login(&self) -> Result<String, AuthenticationUseCaseError> {
        self.oidc()
            .await?
            .begin_login(
                &self.database.oidc_login_attempt_store(),
                &SystemRandom,
                &SystemClock,
            )
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
    }

    async fn complete_login(
        &self,
        code: String,
        state: String,
    ) -> Result<Actor, AuthenticationUseCaseError> {
        let identity = self
            .oidc()
            .await?
            .complete_login(
                &self.database.oidc_login_attempt_store(),
                &SystemClock,
                &code,
                &state,
                "groups",
            )
            .await
            .map_err(|error| match error {
                OidcCallbackError::Rejected(_) => AuthenticationUseCaseError::Rejected,
                OidcCallbackError::Unavailable => AuthenticationUseCaseError::Unavailable,
            })?;
        if !identity.groups.is_user("server-users") {
            return Err(AuthenticationUseCaseError::Rejected);
        }
        marginalis_domain::validate_identity(&identity.issuer, &identity.subject)
            .map_err(|_| AuthenticationUseCaseError::Rejected)?;
        Ok(Actor {
            issuer: identity.issuer,
            subject: identity.subject,
            is_administrator: identity.groups.is_administrator("server-admins"),
        })
    }
}

#[cfg(test)]
mod tests {
    use marginalis_application::{AuthenticationUseCaseError, OidcAuthenticationUseCases};
    use marginalis_auth_oidc::OidcConfiguration;
    use marginalis_sqlite::SqliteDatabase;

    use super::ServerOidcAuthenticationUseCases;

    #[tokio::test]
    async fn unavailability_rejects_login_without_preventing_service_construction() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let configuration = OidcConfiguration::new(
            "https://127.0.0.1:1".into(),
            "marginalis".into(),
            "test-secret".into(),
            "https://marginalis.example.test",
        )
        .expect("configuration");
        let authentication = ServerOidcAuthenticationUseCases::new(
            database,
            configuration,
            reqwest::Client::new(),
            None,
        );
        assert_eq!(
            authentication.begin_login().await,
            Err(AuthenticationUseCaseError::Unavailable)
        );
    }
}
