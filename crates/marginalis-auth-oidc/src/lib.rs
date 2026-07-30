//! OIDC provider接続の設定境界。HTTP handlerやSQLiteへは依存しない。

use std::{collections::BTreeSet, sync::Arc};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    Clock, ExternalIdentity, IdentityProvider, IdentityProviderError, OidcLoginAttempt,
    OidcLoginAttemptStore, Random,
};
use marginalis_domain::UnixMillis;
use openidconnect::{
    AuthType, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreJwsSigningAlgorithm, CoreProviderMetadata},
    reqwest,
};
use serde::Deserialize;
use url::Url;

const MAX_ID_TOKEN_BYTES: usize = 16 * 1024;
const MAX_GROUPS_PER_ID_TOKEN: usize = 128;
const MAX_GROUP_NAME_BYTES: usize = 256;

fn allowed_id_token_algorithms(
    supported: &[CoreJwsSigningAlgorithm],
) -> Vec<CoreJwsSigningAlgorithm> {
    [CoreJwsSigningAlgorithm::EcdsaP256Sha256]
        .into_iter()
        .filter(|algorithm| supported.contains(algorithm))
        .collect()
}

#[derive(Clone)]
pub struct OidcConfiguration {
    issuer_url: IssuerUrl,
    client_id: ClientId,
    client_secret: ClientSecret,
    redirect_url: RedirectUrl,
    cookie_path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OidcConfigurationError {
    #[error("OIDC issuer URL is invalid")]
    InvalidIssuerUrl,
    #[error("Base URL must be an absolute HTTPS URL")]
    InvalidBaseUrl,
}

impl OidcConfiguration {
    pub fn new(
        issuer_url: String,
        client_id: String,
        client_secret: String,
        base_url: &str,
    ) -> Result<Self, OidcConfigurationError> {
        let issuer_url =
            IssuerUrl::new(issuer_url).map_err(|_| OidcConfigurationError::InvalidIssuerUrl)?;
        let redirect_url = callback_url(base_url)?;
        let cookie_path = cookie_path(base_url)?;
        Ok(Self {
            issuer_url,
            client_id: ClientId::new(client_id),
            client_secret: ClientSecret::new(client_secret),
            redirect_url,
            cookie_path,
        })
    }

    pub fn issuer_url(&self) -> &IssuerUrl {
        &self.issuer_url
    }
    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }
    pub fn client_secret(&self) -> &ClientSecret {
        &self.client_secret
    }
    pub fn redirect_url(&self) -> &RedirectUrl {
        &self.redirect_url
    }
    pub fn cookie_path(&self) -> &str {
        &self.cookie_path
    }
}

fn callback_url(base_url: &str) -> Result<RedirectUrl, OidcConfigurationError> {
    let mut url = Url::parse(base_url).map_err(|_| OidcConfigurationError::InvalidBaseUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(OidcConfigurationError::InvalidBaseUrl);
    }
    let base_path = url.path().trim_end_matches('/');
    url.set_path(&format!("{base_path}/auth/oidc/callback"));
    RedirectUrl::new(url.into()).map_err(|_| OidcConfigurationError::InvalidBaseUrl)
}

fn cookie_path(base_url: &str) -> Result<String, OidcConfigurationError> {
    let url = Url::parse(base_url).map_err(|_| OidcConfigurationError::InvalidBaseUrl)?;
    let path = url.path().trim_end_matches('/');
    Ok(if path.is_empty() {
        "/".into()
    } else {
        path.into()
    })
}

/// Discovery済みの外部OIDCクライアント。
pub type DiscoveredOidcClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// OIDC providerとの通信を担うadapter。
///
/// SQLiteやHTTP frameworkには依存せず、login attemptとidentityの永続化はapplication portを
/// 通じて行う。
#[derive(Clone)]
pub struct OidcAuthentication {
    client: DiscoveredOidcClient,
    http_client: reqwest::Client,
    cookie_path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OidcDiscoveryError {
    #[error("OIDC HTTP client could not be initialized")]
    HttpClient,
    #[error("OIDC Discovery failed")]
    Discovery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OidcLoginStartError {
    Store,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OidcCallbackError {
    Rejected(OidcCallbackRejection),
    Unavailable,
}

/// OAuth callbackの安全に記録できる失敗段階。token、code、stateなどの値は含めない。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OidcCallbackRejection {
    State,
    CodeExchange,
    MissingIdToken,
    Claims,
    Identity,
    Groups,
}

impl OidcCallbackError {
    pub const fn diagnostic_stage(self) -> &'static str {
        match self {
            Self::Rejected(OidcCallbackRejection::State) => "state",
            Self::Rejected(OidcCallbackRejection::CodeExchange) => "code-exchange",
            Self::Rejected(OidcCallbackRejection::MissingIdToken) => "missing-id-token",
            Self::Rejected(OidcCallbackRejection::Claims) => "id-token-claims",
            Self::Rejected(OidcCallbackRejection::Identity) => "identity",
            Self::Rejected(OidcCallbackRejection::Groups) => "groups",
            Self::Unavailable => "storage",
        }
    }
}

/// 署名・issuer・audience・nonceを検証済みのID tokenから得たKanidm group所属。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedOidcGroups {
    groups: BTreeSet<String>,
}

/// v0.3 login callbackでのみ返す、署名検証済みOIDC identityとKanidm group claim。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedOidcIdentity {
    pub issuer: String,
    pub subject: String,
    pub groups: VerifiedOidcGroups,
}

impl VerifiedOidcGroups {
    pub fn is_user(&self, user_group: &str) -> bool {
        self.groups.contains(user_group)
    }

    pub fn into_names(self) -> Vec<String> {
        self.groups.into_iter().collect()
    }
}

#[derive(Deserialize)]
struct GroupClaimPayload {
    #[serde(flatten)]
    claims: serde_json::Map<String, serde_json::Value>,
}

/// 検証済みID tokenのpayloadから、Kanidmで設定した文字列配列のgroup claimを読む。
///
/// この関数はJWTの署名検証をしない。必ず`IdToken::claims`の成功後にだけ呼び出す。claimが欠落、
/// 文字列配列以外、空のgroup名を含む場合はfail closedで拒否する。
fn groups_from_verified_id_token(
    id_token: &str,
    group_claim: &str,
) -> Result<VerifiedOidcGroups, OidcCallbackRejection> {
    if group_claim.is_empty() || id_token.len() > MAX_ID_TOKEN_BYTES {
        return Err(OidcCallbackRejection::Groups);
    }
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or(OidcCallbackRejection::Groups)?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| OidcCallbackRejection::Groups)?;
    let payload = serde_json::from_slice::<GroupClaimPayload>(&payload)
        .map_err(|_| OidcCallbackRejection::Groups)?;
    let values = payload
        .claims
        .get(group_claim)
        .and_then(serde_json::Value::as_array)
        .ok_or(OidcCallbackRejection::Groups)?;
    if values.len() > MAX_GROUPS_PER_ID_TOKEN {
        return Err(OidcCallbackRejection::Groups);
    }
    let mut groups = BTreeSet::new();
    for value in values {
        let value = value
            .as_str()
            .filter(|value| !value.trim().is_empty() && value.len() <= MAX_GROUP_NAME_BYTES)
            .ok_or(OidcCallbackRejection::Groups)?;
        groups.insert(value.to_owned());
    }
    Ok(VerifiedOidcGroups { groups })
}

impl OidcAuthentication {
    pub async fn discover(configuration: &OidcConfiguration) -> Result<Self, OidcDiscoveryError> {
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|_| OidcDiscoveryError::HttpClient)?;
        Self::discover_with_http_client(configuration, http_client).await
    }

    /// Discoveryとtoken exchangeに使うHTTP clientを明示する。内部CAを使う配備では、
    /// 呼出し側が検証済みPEMをroot certificateとして追加したclientを渡す。
    pub async fn discover_with_http_client(
        configuration: &OidcConfiguration,
        http_client: reqwest::Client,
    ) -> Result<Self, OidcDiscoveryError> {
        let metadata =
            CoreProviderMetadata::discover_async(configuration.issuer_url().clone(), &http_client)
                .await
                .map_err(|_| OidcDiscoveryError::Discovery)?;
        let signing_algorithms =
            allowed_id_token_algorithms(metadata.id_token_signing_alg_values_supported());
        if signing_algorithms.is_empty() {
            return Err(OidcDiscoveryError::Discovery);
        }
        let metadata = metadata.set_id_token_signing_alg_values_supported(signing_algorithms);
        Ok(Self {
            client: CoreClient::from_provider_metadata(
                metadata,
                configuration.client_id().clone(),
                Some(configuration.client_secret().clone()),
            )
            .set_auth_type(AuthType::RequestBody)
            .set_redirect_uri(configuration.redirect_url().clone()),
            http_client,
            cookie_path: configuration.cookie_path().into(),
        })
    }

    pub fn cookie_path(&self) -> &str {
        &self.cookie_path
    }

    pub async fn begin_login<Attempts, Entropy, Time>(
        &self,
        attempts: &Attempts,
        entropy: &Entropy,
        clock: &Time,
    ) -> Result<String, OidcLoginStartError>
    where
        Attempts: OidcLoginAttemptStore,
        Entropy: Random,
        Time: Clock,
    {
        let now = clock.now();
        let pending = OidcLoginAttempt {
            state: entropy.opaque_token(),
            nonce: entropy.opaque_token(),
            pkce_verifier: entropy.opaque_token(),
            expires_at: UnixMillis::new(now.get() + 10 * 60 * 1_000),
        };
        attempts
            .issue(pending.clone(), now)
            .await
            .map_err(|_| OidcLoginStartError::Store)?;
        let verifier = PkceCodeVerifier::new(pending.pkce_verifier);
        let challenge = PkceCodeChallenge::from_code_verifier_sha256(&verifier);
        let state = pending.state;
        let nonce = pending.nonce;
        let (url, _, _) = self
            .client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                move || CsrfToken::new(state),
                move || Nonce::new(nonce),
            )
            .set_pkce_challenge(challenge)
            .add_scope(Scope::new("profile".into()))
            .add_scope(Scope::new("email".into()))
            // Kanidmの`groups_name` scopeは、group名だけからなる文字列配列の`groups` claimを発行する。
            .add_scope(Scope::new("groups_name".into()))
            .url();
        Ok(url.into())
    }

    /// 各adapterを明示的に受け取ることで、OIDC crateを永続化実装から独立させる。
    #[allow(clippy::too_many_arguments)]
    pub async fn complete_login<Attempts, Time>(
        &self,
        attempts: &Attempts,
        clock: &Time,
        code: &str,
        state: &str,
        group_claim: &str,
    ) -> Result<VerifiedOidcIdentity, OidcCallbackError>
    where
        Attempts: OidcLoginAttemptStore,
        Time: Clock,
    {
        let pending = attempts
            .consume(state.to_owned(), clock.now())
            .await
            .map_err(|_| OidcCallbackError::Unavailable)?
            .ok_or(OidcCallbackError::Rejected(OidcCallbackRejection::State))?;
        let token = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .map_err(|_| OidcCallbackError::Rejected(OidcCallbackRejection::CodeExchange))?
            .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier))
            .request_async(&self.http_client)
            .await
            .map_err(|_| OidcCallbackError::Rejected(OidcCallbackRejection::CodeExchange))?;
        let id_token = token.id_token().ok_or(OidcCallbackError::Rejected(
            OidcCallbackRejection::MissingIdToken,
        ))?;
        let claims = id_token
            .claims(&self.client.id_token_verifier(), &Nonce::new(pending.nonce))
            .map_err(|_| OidcCallbackError::Rejected(OidcCallbackRejection::Claims))?;
        let groups = groups_from_verified_id_token(&id_token.to_string(), group_claim)
            .map_err(OidcCallbackError::Rejected)?;
        Ok(VerifiedOidcIdentity {
            issuer: claims.issuer().as_str().to_owned(),
            subject: claims.subject().as_str().to_owned(),
            groups,
        })
    }
}

/// OIDC通信とlogin attempt保存をapplicationのidentity provider portへ接続するadapter。
pub struct OidcIdentityProvider<Attempts, Time, Entropy> {
    attempts: Attempts,
    clock: Time,
    random: Entropy,
    configuration: OidcConfiguration,
    http_client: reqwest::Client,
    discovered: Arc<tokio::sync::RwLock<Option<OidcAuthentication>>>,
}

impl<Attempts, Time, Entropy> OidcIdentityProvider<Attempts, Time, Entropy> {
    pub fn new(
        attempts: Attempts,
        clock: Time,
        random: Entropy,
        configuration: OidcConfiguration,
        http_client: reqwest::Client,
        discovered: Option<OidcAuthentication>,
    ) -> Self {
        Self {
            attempts,
            clock,
            random,
            configuration,
            http_client,
            discovered: Arc::new(tokio::sync::RwLock::new(discovered)),
        }
    }

    async fn oidc(&self) -> Result<OidcAuthentication, IdentityProviderError> {
        if let Some(oidc) = self.discovered.read().await.clone() {
            return Ok(oidc);
        }
        let oidc = OidcAuthentication::discover_with_http_client(
            &self.configuration,
            self.http_client.clone(),
        )
        .await
        .map_err(|_| {
            tracing::warn!(
                event = "oidc.discovery.failed",
                reason = "unavailable",
                "OIDC discovery retry failed"
            );
            IdentityProviderError::Unavailable
        })?;
        tracing::info!(
            event = "oidc.discovery.completed",
            "OIDC discovery succeeded"
        );
        let mut discovered = self.discovered.write().await;
        Ok(discovered.get_or_insert(oidc).clone())
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
        self.oidc()
            .await?
            .begin_login(&self.attempts, &self.random, &self.clock)
            .await
            .map_err(|_| IdentityProviderError::Unavailable)
    }

    async fn complete_login(
        &self,
        code: &str,
        state: &str,
    ) -> Result<ExternalIdentity, IdentityProviderError> {
        let identity = self
            .oidc()
            .await?
            .complete_login(&self.attempts, &self.clock, code, state, "groups")
            .await
            .map_err(|error| match error {
                OidcCallbackError::Rejected(_) => IdentityProviderError::Rejected,
                OidcCallbackError::Unavailable => IdentityProviderError::Unavailable,
            })?;
        Ok(ExternalIdentity {
            issuer: identity.issuer,
            subject: identity.subject,
            groups: identity.groups.into_names(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_base_subpath() {
        let config = OidcConfiguration::new(
            "https://id.example.test".into(),
            "client".into(),
            "secret".into(),
            "https://example.test/app/",
        )
        .expect("config");
        assert_eq!(
            config.redirect_url().as_str(),
            "https://example.test/app/auth/oidc/callback"
        );
        assert_eq!(config.cookie_path(), "/app");
    }

    #[test]
    fn parses_a_configured_group_claim_after_token_verification() {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256"}"#);
        let payload = URL_SAFE_NO_PAD.encode(r#"{"groups":["server-users"]}"#);
        let token = format!("{header}.{payload}.signature");
        let groups = groups_from_verified_id_token(&token, "groups").expect("groups");
        assert!(groups.is_user("server-users"));
    }

    #[test]
    fn rejects_missing_or_non_string_group_claims() {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256"}"#);
        let payload = URL_SAFE_NO_PAD.encode(r#"{"groups":["server-users",3]}"#);
        let token = format!("{header}.{payload}.signature");
        assert_eq!(
            groups_from_verified_id_token(&token, "groups"),
            Err(OidcCallbackRejection::Groups)
        );
    }

    #[test]
    fn id_tokens_are_limited_to_es256() {
        assert_eq!(
            allowed_id_token_algorithms(&[
                CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
                CoreJwsSigningAlgorithm::EcdsaP256Sha256,
                CoreJwsSigningAlgorithm::HmacSha256,
            ]),
            vec![CoreJwsSigningAlgorithm::EcdsaP256Sha256]
        );
        assert!(
            allowed_id_token_algorithms(&[CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256])
                .is_empty()
        );
    }
}
