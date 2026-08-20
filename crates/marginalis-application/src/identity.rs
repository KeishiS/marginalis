//! 外部identity providerを使うログインの業務処理。

use std::{future::Future, sync::Arc};

use async_trait::async_trait;
use marginalis_domain::{
    Actor, AuthenticatedSession, Identity, PrincipalRef, UnixMillis, WebSession,
};

use crate::StorageError;

/// OIDC認可requestに一度だけ対応するstate、nonce、PKCE verifier。
///
/// stateはadapterでhash保存し、nonceとverifierは短い有効期間だけ保持する。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcLoginAttempt {
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub expires_at: UnixMillis,
}

pub trait OidcLoginAttemptStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn issue(
        &self,
        attempt: OidcLoginAttempt,
        now: UnixMillis,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn consume(
        &self,
        state: String,
        now: UnixMillis,
    ) -> impl Future<Output = Result<Option<OidcLoginAttempt>, Self::Error>> + Send;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuthenticationUseCaseError {
    #[error("authentication was rejected")]
    Rejected,
    #[error("authentication is unavailable")]
    Unavailable,
}

/// Kanidm groupはOIDC login時に検証し、このCookie sessionの有効期間はsnapshotとして固定する。
#[async_trait]
pub trait WebSessionUseCases: Send + Sync {
    async fn authenticate_session(
        &self,
        session_id: String,
    ) -> Result<Option<AuthenticatedSession>, AuthenticationUseCaseError>;
    async fn verify_csrf(
        &self,
        session_id: String,
        csrf_token: String,
    ) -> Result<bool, AuthenticationUseCaseError>;
    async fn issue_session(&self, actor: Actor) -> Result<WebSession, AuthenticationUseCaseError>;
    async fn revoke_session(&self, session_id: String) -> Result<(), AuthenticationUseCaseError>;
}

#[async_trait]
pub trait OidcAuthenticationUseCases: Send + Sync {
    async fn begin_login(&self) -> Result<String, AuthenticationUseCaseError>;
    async fn complete_login(
        &self,
        code: String,
        state: String,
    ) -> Result<Actor, AuthenticationUseCaseError>;
}

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

/// 外部identityを内部principalへ安全に対応付ける永続化port。
#[async_trait]
pub trait PrincipalDirectory: Send + Sync {
    /// 検証済みOIDCログインを既存principalへ解決し、初回だけ作成する。
    async fn resolve_or_create_verified(&self, identity: Identity) -> Result<Actor, StorageError>;

    /// sessionまたはtokenの保存値を既存principalへ解決する。未知のidentityは作成しない。
    async fn resolve(&self, identity: &Identity) -> Result<Option<Actor>, StorageError>;

    /// 現在のOIDC issuerに属するACL共有先を解決し、未登録なら作成する。
    async fn resolve_or_create_acl_target(
        &self,
        identity: Identity,
    ) -> Result<PrincipalRef, StorageError>;
}

pub struct OidcAuthenticationApplication {
    provider: Arc<dyn IdentityProvider>,
    principals: Arc<dyn PrincipalDirectory>,
    /// 利用を許可するclaim値の一覧。いずれか1つに一致すれば許可する。
    ///
    /// 空の一覧は「このissuerで認証できた利用者は全員許可」を意味する(ADR 0015)。
    /// IdP側でclientの利用者を絞る運用のための明示的な設定であり、既定値ではない。
    allowed_claim_values: Vec<String>,
}

impl OidcAuthenticationApplication {
    pub fn new(
        provider: Arc<dyn IdentityProvider>,
        principals: Arc<dyn PrincipalDirectory>,
        allowed_claim_values: Vec<String>,
    ) -> Self {
        Self {
            provider,
            principals,
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
        self.principals
            .resolve_or_create_verified(actor_identity)
            .await
            .map_err(|_| AuthenticationUseCaseError::Unavailable)
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use marginalis_domain::PrincipalId;

    use super::*;

    struct FixedProvider {
        groups: Vec<String>,
    }

    struct FixedDirectory {
        creates: AtomicUsize,
    }

    #[async_trait]
    impl PrincipalDirectory for FixedDirectory {
        async fn resolve_or_create_verified(
            &self,
            identity: Identity,
        ) -> Result<Actor, StorageError> {
            self.creates.fetch_add(1, Ordering::Relaxed);
            Ok(Actor::for_single_identity(
                PrincipalId::new(1).expect("ID"),
                identity,
            ))
        }

        async fn resolve(&self, _identity: &Identity) -> Result<Option<Actor>, StorageError> {
            unreachable!("login completion does not restore a session")
        }

        async fn resolve_or_create_acl_target(
            &self,
            _identity: Identity,
        ) -> Result<PrincipalRef, StorageError> {
            unreachable!("login completion does not change an ACL")
        }
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
        application_with_directory(groups, allowed).0
    }

    fn application_with_directory(
        groups: Vec<String>,
        allowed: Vec<String>,
    ) -> (OidcAuthenticationApplication, Arc<FixedDirectory>) {
        let directory = Arc::new(FixedDirectory {
            creates: AtomicUsize::new(0),
        });
        let application = OidcAuthenticationApplication::new(
            Arc::new(FixedProvider { groups }),
            directory.clone(),
            allowed,
        );
        (application, directory)
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

    #[tokio::test]
    async fn a_rejected_allow_list_does_not_create_a_principal() {
        let (application, directory) =
            application_with_directory(vec!["not-allowed".into()], vec!["allowed".into()]);
        assert_eq!(
            application
                .complete_login("code".into(), "state".into())
                .await,
            Err(AuthenticationUseCaseError::Rejected)
        );
        assert_eq!(directory.creates.load(Ordering::Relaxed), 0);
    }
}
