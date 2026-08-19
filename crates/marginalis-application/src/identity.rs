//! 外部identity providerを使うログインの業務処理。

use std::sync::Arc;

use async_trait::async_trait;
use marginalis_domain::{Actor, Identity};

use crate::{AuthenticationUseCaseError, OidcAuthenticationUseCases};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalIdentity {
    pub issuer: String,
    pub subject: String,
    pub groups: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IdentityProviderError {
    #[error("the identity provider rejected the login")]
    Rejected,
    #[error("the identity provider is unavailable")]
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
    /// 利用を許可するclaim値の一覧。いずれか1つに一致すれば許可する。
    ///
    /// 空の一覧は「このissuerで認証できた利用者は全員許可」を意味する(ADR 0015)。
    /// IdP側でclientの利用者を絞る運用のための明示的な設定であり、既定値ではない。
    allowed_claim_values: Vec<String>,
}

impl OidcAuthenticationApplication {
    pub fn new(provider: Arc<dyn IdentityProvider>, allowed_claim_values: Vec<String>) -> Self {
        Self {
            provider,
            allowed_claim_values,
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
        if !self.allowed_claim_values.is_empty()
            && !identity
                .groups
                .iter()
                .any(|group| self.allowed_claim_values.contains(group))
        {
            return Err(AuthenticationUseCaseError::Rejected);
        }
        let actor_identity = Identity::new(identity.issuer, identity.subject)
            .map_err(|_| AuthenticationUseCaseError::Rejected)?;
        Ok(Actor::new(actor_identity))
    }
}

fn map_provider_error(error: IdentityProviderError) -> AuthenticationUseCaseError {
    match error {
        IdentityProviderError::Rejected => AuthenticationUseCaseError::Rejected,
        IdentityProviderError::Unavailable => AuthenticationUseCaseError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedProvider {
        groups: Vec<String>,
    }

    #[async_trait]
    impl IdentityProvider for FixedProvider {
        async fn begin_login(&self) -> Result<String, IdentityProviderError> {
            Ok("https://id.example.test/authorize".into())
        }

        async fn complete_login(
            &self,
            _code: &str,
            _state: &str,
        ) -> Result<ExternalIdentity, IdentityProviderError> {
            Ok(ExternalIdentity {
                issuer: "https://id.example.test".into(),
                subject: "alice".into(),
                groups: self.groups.clone(),
            })
        }
    }

    fn application(groups: Vec<String>, allowed: Vec<String>) -> OidcAuthenticationApplication {
        OidcAuthenticationApplication::new(Arc::new(FixedProvider { groups }), allowed)
    }

    /// 許可値の一覧はいずれか1つに一致すれば許可する。
    #[tokio::test]
    async fn any_allowed_claim_value_grants_access() {
        let allowed = vec!["a@example.com".to_owned(), "b@example.com".to_owned()];
        let granted = application(vec!["b@example.com".into()], allowed.clone());
        assert!(granted.complete_login("c".into(), "s".into()).await.is_ok());

        let denied = application(vec!["c@example.com".into()], allowed);
        assert_eq!(
            denied.complete_login("c".into(), "s".into()).await.err(),
            Some(AuthenticationUseCaseError::Rejected)
        );
    }

    /// 空の一覧は「issuerで認証できた利用者は全員許可」を意味する(ADR 0015)。
    #[tokio::test]
    async fn an_empty_allow_list_permits_every_authenticated_user() {
        let open = application(vec!["anything".into()], Vec::new());
        assert!(open.complete_login("c".into(), "s".into()).await.is_ok());
        let no_groups = application(Vec::new(), Vec::new());
        assert!(
            no_groups
                .complete_login("c".into(), "s".into())
                .await
                .is_ok()
        );
    }
}
