//! 外部Authorization Serverが発行したMCP access tokenの検証adapter。

use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{AlgorithmParameters, Jwk, JwkSet, KeyAlgorithm},
};
use marginalis_application::{
    McpAccessTokenAuthenticationError, McpAccessTokenAuthenticator, McpAccessTokenRejection,
};
use marginalis_domain::{Actor, McpAuthenticatedActor};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use url::Url;

const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_CLAIM_NAME_BYTES: usize = 512;
const MAX_GROUPS: usize = 128;
const MAX_GROUP_BYTES: usize = 256;
const MAX_SCOPES: usize = 16;
const MAX_SCOPE_BYTES: usize = 128;
const MAX_DISCOVERY_RESPONSE_BYTES: usize = 1024 * 1024;
const MINIMUM_JWKS_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const TOKEN_TIME_LEEWAY_SECONDS: u64 = 5;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAuthorizationConfiguration {
    pub issuer: String,
    pub audience: String,
    pub upstream_issuer: String,
    pub upstream_issuer_claim: String,
    pub upstream_subject_claim: String,
    pub groups_claim: String,
    pub required_user_group: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("MCP Authorization Server configuration is invalid")]
pub enum McpAuthorizationConfigurationError {
    InvalidIssuer,
    InvalidAudience,
    InvalidUpstreamIssuer,
    InvalidClaimName,
    InvalidGroup,
}

impl McpAuthorizationConfiguration {
    pub fn validate(self) -> Result<Self, McpAuthorizationConfigurationError> {
        validate_https_url(&self.issuer)
            .map_err(|_| McpAuthorizationConfigurationError::InvalidIssuer)?;
        validate_https_url(&self.audience)
            .map_err(|_| McpAuthorizationConfigurationError::InvalidAudience)?;
        validate_https_url(&self.upstream_issuer)
            .map_err(|_| McpAuthorizationConfigurationError::InvalidUpstreamIssuer)?;
        for claim in [
            &self.upstream_issuer_claim,
            &self.upstream_subject_claim,
            &self.groups_claim,
        ] {
            if claim.is_empty() || claim.len() > MAX_CLAIM_NAME_BYTES {
                return Err(McpAuthorizationConfigurationError::InvalidClaimName);
            }
        }
        if self.required_user_group.trim().is_empty()
            || self.required_user_group.len() > MAX_GROUP_BYTES
        {
            return Err(McpAuthorizationConfigurationError::InvalidGroup);
        }
        Ok(self)
    }
}

fn validate_https_url(value: &str) -> Result<(), ()> {
    let url = Url::parse(value).map_err(|_| ())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(());
    }
    Ok(())
}

#[derive(Deserialize)]
struct AuthorizationServerMetadata {
    issuer: String,
    jwks_uri: String,
}

#[derive(Deserialize)]
struct Claims {
    #[serde(flatten)]
    values: serde_json::Map<String, serde_json::Value>,
}

pub struct McpAccessTokenAuthenticatorAdapter {
    configuration: McpAuthorizationConfiguration,
    client: reqwest::Client,
    jwks_uri: Url,
    jwks: Arc<RwLock<JwkSet>>,
    jwks_refresh: Arc<Mutex<()>>,
    last_jwks_refresh: Arc<RwLock<JwksRefreshState>>,
}

#[derive(Clone, Copy)]
enum JwksRefreshState {
    Successful(Instant),
    Failed(Instant, McpAccessTokenAuthenticationError),
}

impl JwksRefreshState {
    fn recent_result(self) -> Option<Result<(), McpAccessTokenAuthenticationError>> {
        match self {
            Self::Successful(at) if at.elapsed() < MINIMUM_JWKS_REFRESH_INTERVAL => Some(Ok(())),
            Self::Failed(at, error) if at.elapsed() < MINIMUM_JWKS_REFRESH_INTERVAL => {
                Some(Err(error))
            }
            Self::Successful(_) | Self::Failed(_, _) => None,
        }
    }
}

impl McpAccessTokenAuthenticatorAdapter {
    pub async fn discover(
        configuration: McpAuthorizationConfiguration,
    ) -> Result<Self, McpAccessTokenAuthenticationError> {
        let configuration = configuration
            .validate()
            .map_err(|_| McpAccessTokenAuthenticationError::Configuration)?;
        let client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|_| McpAccessTokenAuthenticationError::Configuration)?;
        Self::discover_with_client(configuration, client).await
    }

    pub async fn discover_with_client(
        configuration: McpAuthorizationConfiguration,
        client: reqwest::Client,
    ) -> Result<Self, McpAccessTokenAuthenticationError> {
        let configuration = configuration
            .validate()
            .map_err(|_| McpAccessTokenAuthenticationError::Configuration)?;
        let metadata_url = authorization_server_metadata_url(&configuration.issuer)?;
        let metadata = fetch_json::<AuthorizationServerMetadata>(&client, metadata_url).await?;
        if metadata.issuer != configuration.issuer {
            return Err(McpAccessTokenAuthenticationError::Discovery);
        }
        let jwks_uri = Url::parse(&metadata.jwks_uri)
            .map_err(|_| McpAccessTokenAuthenticationError::Discovery)?;
        let issuer = Url::parse(&configuration.issuer)
            .map_err(|_| McpAccessTokenAuthenticationError::Configuration)?;
        if jwks_uri.scheme() != "https"
            || jwks_uri.host_str().is_none()
            || !jwks_uri.username().is_empty()
            || jwks_uri.password().is_some()
            || jwks_uri.fragment().is_some()
            || jwks_uri.origin() != issuer.origin()
        {
            return Err(McpAccessTokenAuthenticationError::Discovery);
        }
        let jwks = fetch_json::<JwkSet>(&client, jwks_uri.clone()).await?;
        validate_jwks(&jwks)?;
        Ok(Self {
            configuration,
            client,
            jwks_uri,
            jwks: Arc::new(RwLock::new(jwks)),
            jwks_refresh: Arc::new(Mutex::new(())),
            last_jwks_refresh: Arc::new(RwLock::new(JwksRefreshState::Successful(
                Instant::now() - MINIMUM_JWKS_REFRESH_INTERVAL,
            ))),
        })
    }

    async fn decoding_key(
        &self,
        token: &str,
    ) -> Result<Option<DecodingKey>, McpAccessTokenAuthenticationError> {
        let header = decode_header(token).map_err(|_| {
            McpAccessTokenAuthenticationError::Rejected(McpAccessTokenRejection::TokenFormat)
        })?;
        if header.alg != Algorithm::RS256 {
            return Ok(None);
        }
        let Some(key_id) = header.kid.as_deref() else {
            return Ok(None);
        };
        {
            let jwks = self.jwks.read().await;
            if let Some(key) = matching_key(&jwks, key_id) {
                return decoding_key(key).map(Some);
            }
        }
        if let Some(result) = self.last_jwks_refresh.read().await.recent_result() {
            result?;
            return Ok(None);
        }
        let _refresh = self.jwks_refresh.lock().await;
        if let Some(result) = self.last_jwks_refresh.read().await.recent_result() {
            result?;
            return Ok(None);
        }
        let refresh_result = async {
            let refreshed = fetch_json::<JwkSet>(&self.client, self.jwks_uri.clone()).await?;
            validate_jwks(&refreshed)?;
            let key = matching_key(&refreshed, key_id)
                .map(decoding_key)
                .transpose()?;
            Ok::<_, McpAccessTokenAuthenticationError>((refreshed, key))
        }
        .await;
        let (refreshed, key) = match refresh_result {
            Ok(result) => result,
            Err(error) => {
                *self.last_jwks_refresh.write().await =
                    JwksRefreshState::Failed(Instant::now(), error);
                tracing::error!(
                    event = "mcp.authorization.jwks_refresh.failed",
                    reason = authentication_error_reason(error),
                    "MCP Authorization Server signing-key refresh failed"
                );
                return Err(error);
            }
        };
        *self.jwks.write().await = refreshed;
        *self.last_jwks_refresh.write().await = JwksRefreshState::Successful(Instant::now());
        tracing::info!(
            event = "mcp.authorization.jwks_refresh.completed",
            "MCP Authorization Server signing keys were refreshed"
        );
        Ok(key)
    }

    async fn authenticate_token(
        &self,
        token: &str,
        resource_uri: &str,
    ) -> Result<Option<McpAuthenticatedActor>, McpAccessTokenAuthenticationError> {
        if token.len() > MAX_TOKEN_BYTES || resource_uri != self.configuration.audience {
            return Ok(None);
        }
        let Some(key) = self.decoding_key(token).await? else {
            return Ok(None);
        };
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[self.configuration.issuer.as_str()]);
        validation.set_audience(&[self.configuration.audience.as_str()]);
        validation.set_required_spec_claims(&["exp", "iss", "aud"]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = TOKEN_TIME_LEEWAY_SECONDS;
        let token = decode::<Claims>(token, &key, &validation).map_err(|_| {
            McpAccessTokenAuthenticationError::Rejected(McpAccessTokenRejection::StandardClaims)
        })?;
        claims_to_actor(&self.configuration, token.claims).map(Some)
    }
}

fn authentication_error_reason(error: McpAccessTokenAuthenticationError) -> &'static str {
    match error {
        McpAccessTokenAuthenticationError::Configuration => "configuration",
        McpAccessTokenAuthenticationError::Discovery => "discovery",
        McpAccessTokenAuthenticationError::Rejected(reason) => reason.log_reason(),
        McpAccessTokenAuthenticationError::Unavailable => "upstream-unavailable",
    }
}

#[async_trait]
impl McpAccessTokenAuthenticator for McpAccessTokenAuthenticatorAdapter {
    async fn authenticate_access_token(
        &self,
        token: String,
        resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpAccessTokenAuthenticationError> {
        self.authenticate_token(&token, &resource_uri).await
    }
}

fn authorization_server_metadata_url(
    issuer: &str,
) -> Result<Url, McpAccessTokenAuthenticationError> {
    let issuer =
        Url::parse(issuer).map_err(|_| McpAccessTokenAuthenticationError::Configuration)?;
    let mut metadata = issuer.clone();
    let path = issuer.path().trim_end_matches('/');
    let metadata_path = if path.is_empty() {
        "/.well-known/oauth-authorization-server".to_owned()
    } else {
        format!("/.well-known/oauth-authorization-server{path}")
    };
    metadata.set_path(&metadata_path);
    Ok(metadata)
}

async fn fetch_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: Url,
) -> Result<T, McpAccessTokenAuthenticationError> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| McpAccessTokenAuthenticationError::Unavailable)?;
    if !response.status().is_success() {
        return Err(McpAccessTokenAuthenticationError::Discovery);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DISCOVERY_RESPONSE_BYTES as u64)
    {
        return Err(McpAccessTokenAuthenticationError::Discovery);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| McpAccessTokenAuthenticationError::Unavailable)?
    {
        if body.len().saturating_add(chunk.len()) > MAX_DISCOVERY_RESPONSE_BYTES {
            return Err(McpAccessTokenAuthenticationError::Discovery);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| McpAccessTokenAuthenticationError::Discovery)
}

fn matching_key<'a>(jwks: &'a JwkSet, key_id: &str) -> Option<&'a Jwk> {
    jwks.keys.iter().find(|key| {
        key.common.key_id.as_deref() == Some(key_id)
            && key
                .common
                .key_algorithm
                .is_none_or(|algorithm| algorithm == KeyAlgorithm::RS256)
            && key
                .common
                .public_key_use
                .as_ref()
                .is_none_or(|usage| matches!(usage, jsonwebtoken::jwk::PublicKeyUse::Signature))
    })
}

fn validate_jwks(jwks: &JwkSet) -> Result<(), McpAccessTokenAuthenticationError> {
    let mut key_ids = BTreeSet::new();
    for key in &jwks.keys {
        let is_rs256_signing_key =
            key.common
                .key_algorithm
                .is_none_or(|algorithm| algorithm == KeyAlgorithm::RS256)
                && key.common.public_key_use.as_ref().is_none_or(|usage| {
                    matches!(usage, jsonwebtoken::jwk::PublicKeyUse::Signature)
                });
        if !is_rs256_signing_key {
            continue;
        }
        let rsa_parameters_are_present = matches!(
            &key.algorithm,
            AlgorithmParameters::RSA(parameters)
                if !parameters.n.is_empty() && !parameters.e.is_empty()
        );
        let Some(key_id) = key.common.key_id.as_ref() else {
            return Err(McpAccessTokenAuthenticationError::Discovery);
        };
        if !rsa_parameters_are_present
            || DecodingKey::from_jwk(key).is_err()
            || !key_ids.insert(key_id)
        {
            return Err(McpAccessTokenAuthenticationError::Discovery);
        }
    }
    if key_ids.is_empty() {
        Err(McpAccessTokenAuthenticationError::Discovery)
    } else {
        Ok(())
    }
}

fn decoding_key(key: &Jwk) -> Result<DecodingKey, McpAccessTokenAuthenticationError> {
    DecodingKey::from_jwk(key).map_err(|_| McpAccessTokenAuthenticationError::Discovery)
}

fn claims_to_actor(
    configuration: &McpAuthorizationConfiguration,
    claims: Claims,
) -> Result<McpAuthenticatedActor, McpAccessTokenAuthenticationError> {
    let upstream_issuer = claim_string(&claims, &configuration.upstream_issuer_claim)?;
    if upstream_issuer != configuration.upstream_issuer {
        return Err(McpAccessTokenAuthenticationError::Rejected(
            McpAccessTokenRejection::IdentityClaims,
        ));
    }
    let upstream_subject = claim_string(&claims, &configuration.upstream_subject_claim)?;
    let groups = claim_strings(
        &claims,
        &configuration.groups_claim,
        MAX_GROUPS,
        MAX_GROUP_BYTES,
    )?;
    if !groups.contains(&configuration.required_user_group) {
        return Err(McpAccessTokenAuthenticationError::Rejected(
            McpAccessTokenRejection::GroupsClaim,
        ));
    }
    let scopes = claim_scope(&claims)?;
    let actor =
        Actor::try_new(upstream_issuer.to_owned(), upstream_subject.to_owned()).map_err(|_| {
            McpAccessTokenAuthenticationError::Rejected(McpAccessTokenRejection::IdentityClaims)
        })?;
    Ok(McpAuthenticatedActor { actor, scopes })
}

fn claim_string<'a>(
    claims: &'a Claims,
    name: &str,
) -> Result<&'a str, McpAccessTokenAuthenticationError> {
    claims
        .values
        .get(name)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(McpAccessTokenAuthenticationError::Rejected(
            McpAccessTokenRejection::IdentityClaims,
        ))
}

fn claim_strings(
    claims: &Claims,
    name: &str,
    maximum_values: usize,
    maximum_bytes: usize,
) -> Result<BTreeSet<String>, McpAccessTokenAuthenticationError> {
    let values = claims
        .values
        .get(name)
        .and_then(serde_json::Value::as_array)
        .filter(|values| values.len() <= maximum_values)
        .ok_or(McpAccessTokenAuthenticationError::Rejected(
            McpAccessTokenRejection::GroupsClaim,
        ))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty() && value.len() <= maximum_bytes)
                .map(str::to_owned)
                .ok_or(McpAccessTokenAuthenticationError::Rejected(
                    McpAccessTokenRejection::GroupsClaim,
                ))
        })
        .collect()
}

fn claim_scope(claims: &Claims) -> Result<Vec<String>, McpAccessTokenAuthenticationError> {
    let scopes = claims
        .values
        .get("scope")
        .and_then(serde_json::Value::as_str)
        .ok_or(McpAccessTokenAuthenticationError::Rejected(
            McpAccessTokenRejection::ScopeClaim,
        ))?
        .split_ascii_whitespace()
        .collect::<BTreeSet<_>>();
    if scopes.is_empty()
        || scopes.len() > MAX_SCOPES
        || scopes.iter().any(|scope| {
            scope.len() > MAX_SCOPE_BYTES
                || !matches!(
                    *scope,
                    "notes:read" | "notes:write" | "notes:delete" | "offline_access"
                )
        })
    {
        return Err(McpAccessTokenAuthenticationError::Rejected(
            McpAccessTokenRejection::ScopeClaim,
        ));
    }
    Ok(scopes
        .into_iter()
        .filter(|scope| scope.starts_with("notes:"))
        .map(str::to_owned)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use jsonwebtoken::{EncodingKey, Header, encode};
    use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};

    fn configuration() -> McpAuthorizationConfiguration {
        McpAuthorizationConfiguration {
            issuer: "https://evaluation.jp.auth0.com/".into(),
            audience: "https://notes.example.test/mcp".into(),
            upstream_issuer: "https://id.example.test/oauth2/openid/marginalis".into(),
            upstream_issuer_claim: "https://notes.example.test/upstream_issuer".into(),
            upstream_subject_claim: "https://notes.example.test/upstream_subject".into(),
            groups_claim: "https://notes.example.test/groups".into(),
            required_user_group: "server-users".into(),
        }
    }

    fn claims(
        upstream_issuer: serde_json::Value,
        upstream_subject: serde_json::Value,
        groups: serde_json::Value,
        scope: serde_json::Value,
    ) -> Claims {
        let configuration = configuration();
        Claims {
            values: serde_json::Map::from_iter([
                (configuration.upstream_issuer_claim, upstream_issuer),
                (configuration.upstream_subject_claim, upstream_subject),
                (configuration.groups_claim, groups),
                ("scope".into(), scope),
            ]),
        }
    }

    #[test]
    fn configuration_requires_https_identity_and_bounded_claim_names() {
        assert!(configuration().validate().is_ok());

        let mut invalid = configuration();
        invalid.issuer = "http://evaluation.example.test".into();
        assert_eq!(
            invalid.validate(),
            Err(McpAuthorizationConfigurationError::InvalidIssuer)
        );

        let mut invalid = configuration();
        invalid.groups_claim = "x".repeat(MAX_CLAIM_NAME_BYTES + 1);
        assert_eq!(
            invalid.validate(),
            Err(McpAuthorizationConfigurationError::InvalidClaimName)
        );
    }

    #[test]
    fn verified_claims_preserve_the_upstream_owner_and_scopes() {
        let configuration = configuration();
        let authenticated = claims_to_actor(
            &configuration,
            claims(
                serde_json::json!(configuration.upstream_issuer),
                serde_json::json!("user-a"),
                serde_json::json!(["server-users", "server-admins"]),
                serde_json::json!("notes:write offline_access notes:read"),
            ),
        )
        .expect("valid claims");

        assert_eq!(
            authenticated.actor.issuer(),
            "https://id.example.test/oauth2/openid/marginalis"
        );
        assert_eq!(authenticated.actor.subject(), "user-a");
        assert_eq!(
            authenticated.scopes,
            vec!["notes:read".to_owned(), "notes:write".to_owned()]
        );
    }

    #[test]
    fn claims_reject_identity_group_and_scope_ambiguity() {
        let configuration = configuration();
        for rejected in [
            claims(
                serde_json::json!("https://other.example.test"),
                serde_json::json!("user-a"),
                serde_json::json!(["server-users"]),
                serde_json::json!("notes:read"),
            ),
            claims(
                serde_json::json!(configuration.upstream_issuer),
                serde_json::json!("user-a"),
                serde_json::json!(["server-admins"]),
                serde_json::json!("notes:read"),
            ),
            claims(
                serde_json::json!(configuration.upstream_issuer),
                serde_json::json!("user-a"),
                serde_json::json!(["server-users"]),
                serde_json::json!("notes:read administrator"),
            ),
            claims(
                serde_json::json!(configuration.upstream_issuer),
                serde_json::json!(["user-a"]),
                serde_json::json!(["server-users"]),
                serde_json::json!("notes:read"),
            ),
        ] {
            assert!(matches!(
                claims_to_actor(&configuration, rejected),
                Err(McpAccessTokenAuthenticationError::Rejected(_))
            ));
        }
    }

    #[test]
    fn metadata_url_follows_rfc_8414_issuer_path_rules() {
        assert_eq!(
            authorization_server_metadata_url("https://evaluation.jp.auth0.com/")
                .expect("metadata URL")
                .as_str(),
            "https://evaluation.jp.auth0.com/.well-known/oauth-authorization-server"
        );
        assert_eq!(
            authorization_server_metadata_url("https://example.test/tenant")
                .expect("metadata URL")
                .as_str(),
            "https://example.test/.well-known/oauth-authorization-server/tenant"
        );
    }

    #[tokio::test]
    async fn failed_jwks_refresh_is_not_retried_for_each_unknown_key() {
        let token = format!(
            "{}.{}.signature",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"unknown"}"#),
            URL_SAFE_NO_PAD.encode(br#"{}"#)
        );
        let authenticator = McpAccessTokenAuthenticatorAdapter {
            configuration: configuration(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_millis(100))
                .build()
                .expect("HTTP client"),
            jwks_uri: Url::parse("http://127.0.0.1:9/jwks").expect("JWKS URL"),
            jwks: Arc::new(RwLock::new(JwkSet { keys: Vec::new() })),
            jwks_refresh: Arc::new(Mutex::new(())),
            last_jwks_refresh: Arc::new(RwLock::new(JwksRefreshState::Successful(
                Instant::now() - MINIMUM_JWKS_REFRESH_INTERVAL,
            ))),
        };

        assert!(matches!(
            authenticator.decoding_key(&token).await,
            Err(McpAccessTokenAuthenticationError::Unavailable)
        ));
        assert!(matches!(
            authenticator.decoding_key(&token).await,
            Err(McpAccessTokenAuthenticationError::Unavailable)
        ));
    }

    #[test]
    fn jwks_requires_a_usable_rs256_signing_key() {
        assert_eq!(
            validate_jwks(&JwkSet { keys: Vec::new() }),
            Err(McpAccessTokenAuthenticationError::Discovery)
        );
        let unusable = serde_json::from_value(serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "kid": "broken",
                "use": "sig",
                "alg": "RS256",
                "n": "",
                "e": ""
            }]
        }))
        .expect("JWKS");
        assert_eq!(
            validate_jwks(&unusable),
            Err(McpAccessTokenAuthenticationError::Discovery)
        );

        let private_key = RsaPrivateKey::new(&mut rand::thread_rng(), 2_048).expect("test RSA key");
        let public_key = private_key.to_public_key();
        let duplicate: JwkSet = serde_json::from_value(serde_json::json!({
            "keys": [
                {
                    "kty": "RSA",
                    "kid": "duplicate",
                    "use": "sig",
                    "alg": "RS256",
                    "n": URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be()),
                    "e": URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be())
                },
                {
                    "kty": "RSA",
                    "kid": "duplicate",
                    "use": "sig",
                    "alg": "RS256",
                    "n": URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be()),
                    "e": URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be())
                }
            ]
        }))
        .expect("duplicate JWKS");
        assert_eq!(
            validate_jwks(&duplicate),
            Err(McpAccessTokenAuthenticationError::Discovery)
        );
    }

    #[tokio::test]
    async fn rs256_signature_issuer_audience_and_expiry_are_verified() {
        let private_key = RsaPrivateKey::new(&mut rand::thread_rng(), 2_048).expect("test RSA key");
        let private_der = private_key.to_pkcs1_der().expect("private key DER");
        let public_key = private_key.to_public_key();
        let jwks: JwkSet = serde_json::from_value(serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "kid": "evaluation-key",
                "use": "sig",
                "alg": "RS256",
                "n": URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be()),
                "e": URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be())
            }]
        }))
        .expect("JWKS");
        let configuration = configuration();
        let authenticator = McpAccessTokenAuthenticatorAdapter {
            configuration: configuration.clone(),
            client: reqwest::Client::new(),
            jwks_uri: Url::parse("https://evaluation.jp.auth0.com/.well-known/jwks.json")
                .expect("JWKS URL"),
            jwks: Arc::new(RwLock::new(jwks)),
            jwks_refresh: Arc::new(Mutex::new(())),
            last_jwks_refresh: Arc::new(RwLock::new(JwksRefreshState::Successful(Instant::now()))),
        };
        let now = jsonwebtoken::get_current_timestamp();
        let token_claims = serde_json::json!({
            "iss": configuration.issuer,
            "aud": configuration.audience,
            "exp": now + 300,
            "iat": now,
            "scope": "notes:read notes:write",
            configuration.upstream_issuer_claim:
                "https://id.example.test/oauth2/openid/marginalis",
            configuration.upstream_subject_claim: "user-a",
            configuration.groups_claim: ["server-users"]
        });
        let token = encode(
            &Header {
                alg: Algorithm::RS256,
                kid: Some("evaluation-key".into()),
                ..Header::default()
            },
            &token_claims,
            &EncodingKey::from_rsa_der(private_der.as_bytes()),
        )
        .expect("signed token");
        let authenticated = authenticator
            .authenticate_token(&token, "https://notes.example.test/mcp")
            .await
            .expect("token verification")
            .expect("authenticated token");
        assert_eq!(authenticated.actor.subject(), "user-a");

        let mut wrong_audience_claims = token_claims.clone();
        wrong_audience_claims["aud"] = serde_json::json!("https://other.example.test/mcp");
        let wrong_audience_token = encode(
            &Header {
                alg: Algorithm::RS256,
                kid: Some("evaluation-key".into()),
                ..Header::default()
            },
            &wrong_audience_claims,
            &EncodingKey::from_rsa_der(private_der.as_bytes()),
        )
        .expect("wrong-audience token");
        assert_eq!(
            authenticator
                .authenticate_access_token(
                    wrong_audience_token,
                    "https://notes.example.test/mcp".into()
                )
                .await,
            Err(McpAccessTokenAuthenticationError::Rejected(
                McpAccessTokenRejection::StandardClaims
            ))
        );

        let mut expired_claims = token_claims;
        expired_claims["exp"] =
            serde_json::json!(now.saturating_sub(TOKEN_TIME_LEEWAY_SECONDS + 1));
        let expired_token = encode(
            &Header {
                alg: Algorithm::RS256,
                kid: Some("evaluation-key".into()),
                ..Header::default()
            },
            &expired_claims,
            &EncodingKey::from_rsa_der(private_der.as_bytes()),
        )
        .expect("expired token");
        assert_eq!(
            authenticator
                .authenticate_access_token(expired_token, configuration.audience.clone())
                .await,
            Err(McpAccessTokenAuthenticationError::Rejected(
                McpAccessTokenRejection::StandardClaims
            ))
        );

        assert!(
            authenticator
                .authenticate_token(&token, "https://other.example.test/mcp")
                .await
                .expect("resource rejection")
                .is_none()
        );
    }
}
