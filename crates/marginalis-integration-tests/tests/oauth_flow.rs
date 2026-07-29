//! OIDC group認可、Web session、MCP OAuth、SQLiteノート操作を、本番用adapterと
//! Axum routerのHTTP境界を通して一気通貫で検証する。

mod support;

use std::{collections::HashMap, sync::Arc};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{
    Clock, McpOAuthApplication, NoteApplication, OidcAuthenticationApplication,
    OidcAuthenticationUseCases, Random, SessionLifetime, WebSessionApplication,
};
use marginalis_asciidoc::AsciiDocNoteContent;
use marginalis_auth_oidc::{OidcAuthentication, OidcConfiguration, OidcIdentityProvider};
use marginalis_domain::{EntityId, UnixMillis};
use marginalis_integration_tests::MockIdentityProvider;
use marginalis_sqlite::SqliteDatabase;
use marginalis_web::http::{ApiState, McpEndpoint, router};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use url::Url;

use support::mcp_client::{McpTestClient, json_response as mcp_json_response};

const BROWSER_ORIGIN: &str = "https://marginalis.example.test";
const CLIENT_ID: &str = "marginalis-test-client";
const CLIENT_SECRET: &str = "integration-client-secret";
const MCP_RESOURCE: &str = "https://marginalis.example.test/mcp";
const MCP_CALLBACK: &str = "http://localhost:48123/callback";

#[derive(Clone, Copy)]
struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixMillis {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("current time must follow the Unix epoch")
            .as_millis();
        UnixMillis::new(i64::try_from(millis).expect("current time must fit i64 milliseconds"))
    }
}

#[derive(Clone, Copy)]
struct SystemRandom;

impl Random for SystemRandom {
    fn uuid_v7(&self) -> EntityId {
        EntityId::try_from_uuid(uuid::Uuid::now_v7()).expect("Uuid::now_v7 must generate UUIDv7")
    }

    fn opaque_token(&self) -> String {
        URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
    }
}

struct TestServer {
    idp: MockIdentityProvider,
    app: Router,
}

struct BrowserSession {
    session: String,
    csrf: String,
}

struct McpTokens {
    access: String,
    refresh: String,
}

impl BrowserSession {
    fn cookies(&self) -> String {
        format!(
            "marginalis_session={}; marginalis_csrf={}",
            self.session, self.csrf
        )
    }
}

impl TestServer {
    async fn start() -> Self {
        let idp = MockIdentityProvider::start(CLIENT_ID, CLIENT_SECRET).await;
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let configuration = OidcConfiguration::new(
            idp.issuer.clone(),
            CLIENT_ID.into(),
            CLIENT_SECRET.into(),
            BROWSER_ORIGIN,
        )
        .expect("OIDC configuration");
        let discovered = OidcAuthentication::discover(&configuration)
            .await
            .expect("OIDC discovery");
        let provider = OidcIdentityProvider::new(
            database.oidc_login_attempt_store(),
            SystemClock,
            SystemRandom,
            configuration,
            reqwest::Client::new(),
            Some(discovered),
        );
        let oidc = Arc::new(OidcAuthenticationApplication::new(
            Arc::new(provider),
            "server-users",
        ));
        let sessions = Arc::new(WebSessionApplication::new(
            Arc::new(database.clone()),
            Arc::new(SystemClock),
            Arc::new(SystemRandom),
            SessionLifetime {
                idle_timeout_ms: 24 * 60 * 60 * 1_000,
                absolute_timeout_ms: 7 * 24 * 60 * 60 * 1_000,
            },
        ));
        let notes = Arc::new(NoteApplication::new(
            Arc::new(database.clone()),
            Arc::new(database.clone()),
            Arc::new(database.clone()),
            Arc::new(AsciiDocNoteContent),
            Arc::new(marginalis_web::http::HttpNoteLinkResolver),
            Arc::new(SystemClock),
            Arc::new(SystemRandom),
        ));
        let oauth = Arc::new(McpOAuthApplication::new(
            Arc::new(database),
            Arc::new(SystemClock),
            Arc::new(SystemRandom),
            MCP_RESOURCE.into(),
        ));
        let base_url = url::Url::parse(BROWSER_ORIGIN).expect("base URL");
        let state =
            ApiState::new(notes, sessions, oidc, "/".into(), BROWSER_ORIGIN.into()).with_mcp(
                McpEndpoint::new(oauth, &base_url, vec!["https://chatgpt.com".into()]),
            );
        Self {
            idp,
            app: router(state),
        }
    }
}

async fn send(app: &Router, request: Request<Body>) -> Response<Body> {
    app.clone().oneshot(request).await.expect("HTTP response")
}

async fn json_body(response: Response<Body>) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("JSON response")
}

async fn text_body(response: Response<Body>) -> String {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    String::from_utf8(bytes.to_vec()).expect("UTF-8 response")
}

fn cookie(response: &Response<Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|value| {
            let value = value.to_str().ok()?;
            let (key, value) = value.split(';').next()?.split_once('=')?;
            (key == name && !value.is_empty()).then(|| value.to_owned())
        })
}

async fn login_response(
    server: &TestServer,
    subject: &str,
    groups: &[&str],
    code: &str,
) -> Response<Body> {
    let response = send(
        &server.app,
        Request::get("/auth/oidc/login")
            .body(Body::empty())
            .expect("login request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    let authorization = Url::parse(
        response
            .headers()
            .get(header::LOCATION)
            .expect("authorization URL")
            .to_str()
            .expect("location"),
    )
    .expect("authorization URL");
    let query = authorization
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<HashMap<_, _>>();
    let state = query.get("state").expect("state");
    server.idp.approve_with_groups(
        code,
        subject,
        query.get("nonce").expect("nonce"),
        query.get("code_challenge").expect("OIDC PKCE challenge"),
        groups.iter().map(|group| (*group).to_owned()).collect(),
    );
    let callback = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("code", code)
        .append_pair("state", state)
        .finish();
    send(
        &server.app,
        Request::get(format!("/auth/oidc/callback?{callback}"))
            .body(Body::empty())
            .expect("callback request"),
    )
    .await
}

async fn login(server: &TestServer, subject: &str, groups: &[&str], code: &str) -> BrowserSession {
    let response = login_response(server, subject, groups, code).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    BrowserSession {
        session: cookie(&response, "marginalis_session").expect("session cookie"),
        csrf: cookie(&response, "marginalis_csrf").expect("CSRF cookie"),
    }
}

async fn register_mcp_client(app: &Router) -> String {
    let response = send(
        app,
        Request::post("/oauth/register")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "client_name": "Claude Code integration test",
                    "redirect_uris": [MCP_CALLBACK],
                })
                .to_string(),
            ))
            .expect("registration request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await["client_id"]
        .as_str()
        .expect("client ID")
        .to_owned()
}

async fn assert_cross_origin_authorization_post_starts_login(app: &Router, client_id: &str) {
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(
        b"integration-pkce-verifier-with-more-than-forty-three-characters",
    ));
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", MCP_CALLBACK)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("scope", "notes:read notes:write notes:delete")
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", "cross-origin-client-state")
        .finish();
    let response = send(
        app,
        Request::post(format!("/oauth/authorize?{query}"))
            .header(header::ORIGIN, "https://chatgpt.com")
            .header("sec-fetch-site", "cross-site")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("csrf_token=client-owned-value"))
            .expect("cross-origin authorization request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(
        response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|location| location.starts_with("/auth/oidc/login?next="))
    );
}

async fn authorize_mcp(app: &Router, browser: &BrowserSession, client_id: &str) -> McpTokens {
    authorize_mcp_with_scopes(
        app,
        browser,
        client_id,
        "notes:read notes:write notes:delete",
    )
    .await
}

async fn authorize_mcp_with_scopes(
    app: &Router,
    browser: &BrowserSession,
    client_id: &str,
    scopes: &str,
) -> McpTokens {
    let verifier = "integration-pkce-verifier-with-more-than-forty-three-characters";
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", MCP_CALLBACK)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("scope", scopes)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", "client-state")
        .finish();
    let response = send(
        app,
        Request::get(format!("/oauth/authorize?{query}"))
            .header(header::COOKIE, browser.cookies())
            .body(Body::empty())
            .expect("authorization request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let consent_page = String::from_utf8(
        to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("consent page body")
            .to_vec(),
    )
    .expect("UTF-8 consent page");
    assert!(consent_page.contains("action=\"/oauth/authorize/consent\""));

    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", MCP_CALLBACK)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("scope", scopes)
        .append_pair("code_challenge", &challenge)
        .append_pair("state", "client-state")
        .append_pair("csrf_token", &browser.csrf)
        .append_pair("decision", "approve")
        .finish();
    let response = send(
        app,
        Request::post("/oauth/authorize/consent")
            .header(header::COOKIE, browser.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form))
            .expect("consent request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let redirect = Url::parse(
        response
            .headers()
            .get(header::LOCATION)
            .expect("client redirect")
            .to_str()
            .expect("location"),
    )
    .expect("client redirect");
    let code = redirect
        .query_pairs()
        .find_map(|(key, value)| (key == "code").then(|| value.into_owned()))
        .expect("authorization code");
    assert!(
        redirect
            .query_pairs()
            .any(|(key, value)| key == "state" && value == "client-state")
    );

    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", &code)
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", MCP_CALLBACK)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("code_verifier", verifier)
        .finish();
    let response = send(
        app,
        Request::post("/oauth/token")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form))
            .expect("token request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    McpTokens {
        access: body["access_token"]
            .as_str()
            .expect("access token")
            .to_owned(),
        refresh: body["refresh_token"]
            .as_str()
            .expect("refresh token")
            .to_owned(),
    }
}

async fn call_mcp(
    app: &Router,
    access_token: &str,
    id: u64,
    name: &str,
    arguments: serde_json::Value,
) -> Response<Body> {
    send(
        app,
        Request::post("/mcp")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
            .body(Body::from(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/call",
                    "params": { "name": name, "arguments": arguments },
                })
                .to_string(),
            ))
            .expect("MCP request"),
    )
    .await
}

async fn refresh_mcp(app: &Router, client_id: &str, refresh_token: &str) -> McpTokens {
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("client_id", client_id)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("refresh_token", refresh_token)
        .finish();
    let response = send(
        app,
        Request::post("/oauth/token")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form))
            .expect("refresh request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    McpTokens {
        access: body["access_token"]
            .as_str()
            .expect("refreshed access token")
            .to_owned(),
        refresh: body["refresh_token"]
            .as_str()
            .expect("rotated refresh token")
            .to_owned(),
    }
}

mod full_flow {
    use super::*;

    include!("oauth_flow/full_flow.rs");
}

mod membership {
    use super::*;

    include!("oauth_flow/membership.rs");
}

mod scopes {
    use super::*;

    include!("oauth_flow/scopes.rs");
}

mod discovery {
    use super::*;

    include!("oauth_flow/discovery.rs");
}
