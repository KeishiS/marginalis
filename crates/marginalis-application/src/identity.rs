//! 外部identity providerを使うログインの業務処理。

use std::sync::Arc;

use async_trait::async_trait;
use marginalis_domain::{Actor, validate_identity};

use crate::{AuthenticationUseCaseError, OidcAuthenticationUseCases};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalIdentity {
    pub issuer: String,
    pub subject: String,
    pub groups: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityProviderError {
    Rejected,
    Unavailable,
}

/// OIDCなどの外部認証方式をapplicationへ接続するport。
#[async_trait]
pub trait IdentityProvider: Send + Sync {
    async fn begin_login(&self) -> Result<String, IdentityProviderError>;
    async fn complete_login(
        &self,
        code: &str,
        state: &str,
    ) -> Result<ExternalIdentity, IdentityProviderError>;
}

pub struct OidcAuthenticationApplication {
    provider: Arc<dyn IdentityProvider>,
    user_group: String,
    administrator_group: String,
}

impl OidcAuthenticationApplication {
    pub fn new(
        provider: Arc<dyn IdentityProvider>,
        user_group: impl Into<String>,
        administrator_group: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            user_group: user_group.into(),
            administrator_group: administrator_group.into(),
        }
    }
}

#[async_trait]
impl OidcAuthenticationUseCases for OidcAuthenticationApplication {
    async fn begin_login(&self) -> Result<String, AuthenticationUseCaseError> {
        self.provider
            .begin_login()
            .await
            .map_err(map_provider_error)
    }

    async fn complete_login(
        &self,
        code: String,
        state: String,
    ) -> Result<Actor, AuthenticationUseCaseError> {
        let identity = self
            .provider
            .complete_login(&code, &state)
            .await
            .map_err(map_provider_error)?;
        if !identity
            .groups
            .iter()
            .any(|group| group == &self.user_group)
        {
            return Err(AuthenticationUseCaseError::Rejected);
        }
        validate_identity(&identity.issuer, &identity.subject)
            .map_err(|_| AuthenticationUseCaseError::Rejected)?;
        Ok(Actor {
            issuer: identity.issuer,
            subject: identity.subject,
            is_administrator: identity
                .groups
                .iter()
                .any(|group| group == &self.administrator_group),
        })
    }
}

fn map_provider_error(error: IdentityProviderError) -> AuthenticationUseCaseError {
    match error {
        IdentityProviderError::Rejected => AuthenticationUseCaseError::Rejected,
        IdentityProviderError::Unavailable => AuthenticationUseCaseError::Unavailable,
    }
}
