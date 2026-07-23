//! E2E第一層（in-process統合試験）の支援コード。
//!
//! このcrateは試験専用であり、本番crateから参照してはならない。試験用のOIDC providerを
//! 実HTTP listenerとして提供し、`openidconnect`のDiscovery・code交換・ID token検証を
//! ネットワーク越しに通す。第二層（NixOS VM上の実Kanidm）はIssue 030で別途扱う。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

/// 認可承認済みとして登録された、一回限りのauthorization code。
struct PendingAuthorization {
    subject: String,
    nonce: String,
    code_challenge: String,
}

struct MockIdpState {
    issuer: String,
    client_id: String,
    client_secret: String,
    codes: HashMap<String, PendingAuthorization>,
}

/// HS256でID tokenを署名する試験用OIDC provider。
///
/// 認可endpointへの実アクセスは想定しない。試験側が認可要求URLからstate・nonce・
/// code challengeを読み取り、[`MockIdentityProvider::approve`]でcodeを登録することで
/// 利用者の承認を模擬する。
pub struct MockIdentityProvider {
    /// Discovery文書のissuer。`http://127.0.0.1:<port>`形式である。
    pub issuer: String,
    state: Arc<Mutex<MockIdpState>>,
}

impl MockIdentityProvider {
    /// 127.0.0.1の空きportでDiscovery・JWKS・token endpointの提供を開始する。
    pub async fn start(client_id: &str, client_secret: &str) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock IdP binds a loopback port");
        let issuer = format!(
            "http://{}",
            listener.local_addr().expect("mock IdP local address")
        );
        let state = Arc::new(Mutex::new(MockIdpState {
            issuer: issuer.clone(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            codes: HashMap::new(),
        }));
        let router = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(jwks))
            .route("/token", post(token))
            .with_state(state.clone());
        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("mock IdP serves");
        });
        Self { issuer, state }
    }

    /// 利用者が認可画面を承認したものとして、一回限りのcodeを登録する。
    pub fn approve(&self, code: &str, subject: &str, nonce: &str, code_challenge: &str) {
        self.state.lock().expect("mock IdP state").codes.insert(
            code.into(),
            PendingAuthorization {
                subject: subject.into(),
                nonce: nonce.into(),
                code_challenge: code_challenge.into(),
            },
        );
    }
}

async fn discovery(State(state): State<Arc<Mutex<MockIdpState>>>) -> Json<serde_json::Value> {
    let issuer = state.lock().expect("mock IdP state").issuer.clone();
    Json(serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["HS256"],
        "scopes_supported": ["openid", "profile", "email"],
        "token_endpoint_auth_methods_supported": ["client_secret_post"],
    }))
}

async fn jwks() -> Json<serde_json::Value> {
    // HS256はclient secretから導出する対称鍵で検証されるため、公開鍵集合は空でよい。
    Json(serde_json::json!({ "keys": [] }))
}

async fn token(
    State(state): State<Arc<Mutex<MockIdpState>>>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let form: HashMap<String, String> = url::form_urlencoded::parse(&body)
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    let mut state = state.lock().expect("mock IdP state");
    if form.get("grant_type").map(String::as_str) != Some("authorization_code")
        || form.get("client_id") != Some(&state.client_id)
        || form.get("client_secret") != Some(&state.client_secret)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let code = form.get("code").ok_or(StatusCode::BAD_REQUEST)?;
    let pending = state.codes.remove(code).ok_or(StatusCode::BAD_REQUEST)?;
    let verifier = form.get("code_verifier").ok_or(StatusCode::BAD_REQUEST)?;
    if URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != pending.code_challenge {
        return Err(StatusCode::BAD_REQUEST);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    let claims = serde_json::json!({
        "iss": state.issuer,
        "sub": pending.subject,
        "aud": state.client_id,
        "exp": now + 3600,
        "iat": now,
        "nonce": pending.nonce,
        "name": "Integration User",
    });
    let id_token = sign_hs256(&state.client_secret, &claims);
    Ok(Json(serde_json::json!({
        "access_token": "mock-access-token",
        "token_type": "Bearer",
        "expires_in": 3600,
        "id_token": id_token,
    })))
}

/// `base64url(header).base64url(payload)`へのHMAC-SHA256署名でHS256 JWTを作る。
fn sign_hs256(secret: &str, claims: &serde_json::Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes());
    let signing_input = format!("{header}.{payload}");
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(signing_input.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    format!("{signing_input}.{signature}")
}
