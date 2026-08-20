//! 共有crate `oidc-browser-login`をMarginalisのapplication portへ接続する薄いadapter。
//!
//! OIDCの検証本体(Authorization Code + PKCE、ID tokenの署名・issuer・audience・nonce検証、
//! fail closedのclaim読み取り)は共有crateが担う(ADR 0015)。このcrateが行うのは、
//! 時刻・乱数・login attempt保存の各portの型写像と、`IdentityProvider` portの実装だけ。

use async_trait::async_trait;
use marginalis_application::{
    Clock, ExternalIdentity, IdentityProvider, IdentityProviderError, OidcLoginAttempt,
    OidcLoginAttemptStore, Random,
};
use marginalis_domain::UnixMillis;
use oidc_browser_login::{CallbackError, LazyOidcLogin};
pub use oidc_browser_login::{
    DiscoveryError as OidcDiscoveryError, OidcLogin as OidcAuthentication,
    OidcSettings as OidcConfiguration, OidcSigningAlgorithm,
    SettingsError as OidcConfigurationError, TokenEndpointAuth as OidcTokenEndpointAuth, reqwest,
};

/// Marginalisの時刻portを共有crateの時刻portへ写す。
struct SharedClock<T>(T);

impl<T: Clock> oidc_browser_login::Clock for SharedClock<T> {
    fn now(&self) -> oidc_browser_login::UnixMillis {
        oidc_browser_login::UnixMillis::new(self.0.now().get())
    }
}

/// Marginalisの乱数portを共有crateの乱数portへ写す。
struct SharedEntropy<R>(R);

impl<R: Random> oidc_browser_login::Entropy for SharedEntropy<R> {
    fn opaque_token(&self) -> String {
        self.0.opaque_token()
    }
}

/// Marginalisのlogin attempt保存portを共有crateの保存portへ写す。
struct SharedAttempts<A>(A);

impl<A: OidcLoginAttemptStore> oidc_browser_login::LoginAttemptStore for SharedAttempts<A> {
    type Error = A::Error;

    async fn issue(
        &self,
        attempt: oidc_browser_login::LoginAttempt,
        now: oidc_browser_login::UnixMillis,
    ) -> Result<(), Self::Error> {
        self.0
            .issue(
                OidcLoginAttempt {
                    state: attempt.state,
                    nonce: attempt.nonce,
                    pkce_verifier: attempt.pkce_verifier,
                    expires_at: UnixMillis::new(attempt.expires_at.get()),
                },
                UnixMillis::new(now.get()),
            )
            .await
    }

    async fn consume(
        &self,
        state: String,
        now: oidc_browser_login::UnixMillis,
    ) -> Result<Option<oidc_browser_login::LoginAttempt>, Self::Error> {
        Ok(self
            .0
            .consume(state, UnixMillis::new(now.get()))
            .await?
            .map(|attempt| oidc_browser_login::LoginAttempt {
                state: attempt.state,
                nonce: attempt.nonce,
                pkce_verifier: attempt.pkce_verifier,
                expires_at: oidc_browser_login::UnixMillis::new(attempt.expires_at.get()),
            }))
    }
}

/// 共有crateのOIDCログインをapplicationのidentity provider portへ接続するadapter。
pub struct OidcIdentityProvider<Attempts, Time, Entropy> {
    login: LazyOidcLogin<SharedAttempts<Attempts>, SharedClock<Time>, SharedEntropy<Entropy>>,
}

impl<Attempts, Time, Entropy> OidcIdentityProvider<Attempts, Time, Entropy>
where
    Attempts: OidcLoginAttemptStore,
    Time: Clock,
    Entropy: Random,
{
    pub fn new(
        attempts: Attempts,
        clock: Time,
        random: Entropy,
        configuration: OidcConfiguration,
        http_client: reqwest::Client,
        discovered: Option<OidcAuthentication>,
    ) -> Self {
        Self {
            login: LazyOidcLogin::new(
                SharedAttempts(attempts),
                SharedClock(clock),
                SharedEntropy(random),
                configuration,
                http_client,
                discovered,
            ),
        }
    }
}

#[async_trait]
impl<Attempts, Time, Entropy> IdentityProvider for OidcIdentityProvider<Attempts, Time, Entropy>
where
    Attempts: OidcLoginAttemptStore + Send + Sync,
    Time: Clock + Send + Sync,
    Entropy: Random + Send + Sync,
{
    async fn begin_login(&self) -> Result<String, IdentityProviderError> {
        self.login
            .begin_login()
            .await
            .map_err(|_| IdentityProviderError::Unavailable)
    }

    async fn complete_login(
        &self,
        code: &str,
        state: &str,
    ) -> Result<ExternalIdentity, IdentityProviderError> {
        let identity =
            self.login
                .complete_login(code, state)
                .await
                .map_err(|error| match error {
                    CallbackError::Rejected(_) => IdentityProviderError::Rejected,
                    CallbackError::Unavailable => IdentityProviderError::Unavailable,
                })?;
        Ok(ExternalIdentity {
            issuer: identity.issuer,
            subject: identity.subject,
            groups: identity.groups.into_names(),
        })
    }
}
