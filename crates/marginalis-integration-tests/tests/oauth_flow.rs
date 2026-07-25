//! v0.3のOIDC group認可、Web session、MCP OAuth、SQLiteノート操作を、本番用adapterと
//! Axum routerのHTTP境界を通して一気通貫で検証する。

use std::{collections::HashMap, sync::Arc};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::{OidcAuthenticationUseCases, SessionLifetime};
use marginalis_auth_oidc::{OidcAuthentication, OidcConfiguration};
use marginalis_integration_tests::MockIdentityProvider;
use marginalis_server::{
    ServerMcpOAuthService, ServerNoteUseCases, ServerOidcAuthenticationUseCases,
    ServerWebSessionUseCases,
};
use marginalis_sqlite::SqliteDatabase;
use marginalis_web::http::{ApiState, McpEndpoint, router};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use url::Url;

const BROWSER_ORIGIN: &str = "https://marginalis.example.test";
const CLIENT_ID: &str = "marginalis-test-client";
const CLIENT_SECRET: &str = "integration-client-secret";
const MCP_RESOURCE: &str = "https://marginalis.example.test/mcp";
const MCP_CALLBACK: &str = "https://chatgpt.com/connector/oauth/callback";

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
        let oidc = Arc::new(ServerOidcAuthenticationUseCases::new(
            database.clone(),
            configuration,
            reqwest::Client::new(),
            Some(discovered),
        ));
        let sessions = Arc::new(ServerWebSessionUseCases::new(
            database.clone(),
            SessionLifetime {
                idle_timeout_ms: 24 * 60 * 60 * 1_000,
                absolute_timeout_ms: 7 * 24 * 60 * 60 * 1_000,
            },
        ));
        let notes = Arc::new(ServerNoteUseCases::new(database.clone()));
        let oauth = Arc::new(ServerMcpOAuthService::new(database, MCP_RESOURCE.into()));
        let state = ApiState::new(
            notes.clone(),
            sessions,
            oidc,
            "/".into(),
            BROWSER_ORIGIN.into(),
        )
        .with_mcp(McpEndpoint {
            oauth,
            notes,
            allowed_origins: vec!["https://chatgpt.com".into()],
            resource_uri: MCP_RESOURCE.into(),
            metadata_uri: format!("{BROWSER_ORIGIN}/.well-known/oauth-protected-resource/mcp"),
            authorization_server_uri: BROWSER_ORIGIN.into(),
            authorization_server_metadata_uri: format!(
                "{BROWSER_ORIGIN}/.well-known/oauth-authorization-server"
            ),
            authorization_endpoint_uri: format!("{BROWSER_ORIGIN}/oauth/authorize"),
            token_endpoint_uri: format!("{BROWSER_ORIGIN}/oauth/token"),
        });
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
                    "client_name": "ChatGPT integration test",
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

async fn authorize_mcp(app: &Router, browser: &BrowserSession, client_id: &str) -> McpTokens {
    let verifier = "integration-pkce-verifier-with-more-than-forty-three-characters";
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", MCP_CALLBACK)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("scope", "notes:read notes:write notes:delete")
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

    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", MCP_CALLBACK)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("scope", "notes:read notes:write notes:delete")
        .append_pair("code_challenge", &challenge)
        .append_pair("state", "client-state")
        .append_pair("csrf_token", &browser.csrf)
        .append_pair("decision", "approve")
        .finish();
    let response = send(
        app,
        Request::post("/oauth/authorize")
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

#[tokio::test]
async fn oidc_mcp_and_revocation_form_one_http_flow() {
    let server = TestServer::start().await;
    let client_id = register_mcp_client(&server.app).await;
    let user = login(
        &server,
        "user-subject",
        &["server-users"],
        "user-login-code",
    )
    .await;
    let tokens = authorize_mcp(&server.app, &user, &client_id).await;

    let response = call_mcp(
        &server.app,
        &tokens.access,
        1,
        "create_note",
        serde_json::json!({
            "title": "v0.3 <integration> note",
            "body": "Created through the authenticated MCP endpoint.",
            "tags": ["integration"],
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let created = json_body(response).await;
    let note_id = created["result"]["structuredContent"]["note_id"]
        .as_str()
        .expect("created note ID");

    let response = send(
        &server.app,
        Request::get("/api/v2/notes")
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("REST list request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let notes = json_body(response).await;
    assert_eq!(notes.as_array().expect("notes").len(), 1);

    let response = send(
        &server.app,
        Request::get("/api/v2/session")
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("session request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let session = json_body(response).await;
    assert_eq!(session["subject"], "user-subject");
    assert_eq!(session["is_administrator"], false);

    let response = send(
        &server.app,
        Request::get("/")
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("UI list request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = text_body(response).await;
    assert!(body.contains("v0.3 &lt;integration&gt; note"));
    assert!(!body.contains("<integration>"));

    let response = send(
        &server.app,
        Request::get(format!("/notes/{note_id}"))
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("UI note request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        text_body(response)
            .await
            .contains("Created through the authenticated MCP endpoint.")
    );

    let response = send(
        &server.app,
        Request::get(format!("/api/v2/notes/{note_id}"))
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("REST read request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let note = json_body(response).await;
    let revision = note["revision"].as_i64().expect("revision");

    let response = send(
        &server.app,
        Request::put(format!("/api/v2/notes/{note_id}"))
            .header(header::COOKIE, user.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", &user.csrf)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "title": "Updated integration note",
                    "body": "Updated through REST.",
                    "tags": ["integration", "updated"],
                    "expected_revision": revision,
                })
                .to_string(),
            ))
            .expect("REST update request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated = json_body(response).await;
    let revision = updated["revision"].as_i64().expect("updated revision");

    let response = send(
        &server.app,
        Request::get(format!("/api/v2/notes/{note_id}/source"))
            .header(header::COOKIE, user.cookies())
            .body(Body::empty())
            .expect("REST export request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        text_body(response)
            .await
            .contains("= Updated integration note")
    );

    let response = send(
        &server.app,
        Request::delete(format!("/api/v2/notes/{note_id}"))
            .header(header::COOKIE, user.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", &user.csrf)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "expected_revision": revision }).to_string(),
            ))
            .expect("REST delete request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let deleted = json_body(response).await;
    let revision = deleted["revision"].as_i64().expect("deleted revision");

    let response = send(
        &server.app,
        Request::post(format!("/api/v2/notes/{note_id}/restore"))
            .header(header::COOKIE, user.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", &user.csrf)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "expected_revision": revision }).to_string(),
            ))
            .expect("REST restore request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["title"],
        "Updated integration note"
    );

    let administrator = login(
        &server,
        "administrator-subject",
        &["server-users", "server-admins"],
        "administrator-login-code",
    )
    .await;
    let response = send(
        &server.app,
        Request::get("/api/v2/notes")
            .header(header::COOKIE, administrator.cookies())
            .body(Body::empty())
            .expect("administrator list request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response)
            .await
            .as_array()
            .expect("administrator notes")
            .len(),
        1
    );

    let response = send(
        &server.app,
        Request::delete(format!("/api/v2/mcp-authorizations/{client_id}"))
            .header(header::COOKIE, user.cookies())
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header("x-csrf-token", &user.csrf)
            .body(Body::empty())
            .expect("revocation request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = call_mcp(
        &server.app,
        &tokens.access,
        2,
        "list_notes",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("client_id", &client_id)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("refresh_token", &tokens.refresh)
        .finish();
    let response = send(
        &server.app,
        Request::post("/oauth/token")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form))
            .expect("revoked refresh request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid_grant");
}

#[tokio::test]
async fn oidc_rejects_a_subject_without_server_users_membership() {
    let server = TestServer::start().await;
    let response = login_response(
        &server,
        "administrator-only-subject",
        &["server-admins"],
        "rejected-login-code",
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(cookie(&response, "marginalis_session").is_none());
}

#[tokio::test]
async fn oidc_discovery_is_retried_with_the_configured_http_client() {
    let idp = MockIdentityProvider::start(CLIENT_ID, CLIENT_SECRET).await;
    let database = SqliteDatabase::connect("sqlite::memory:")
        .await
        .expect("database");
    let configuration = OidcConfiguration::new(
        idp.issuer,
        CLIENT_ID.into(),
        CLIENT_SECRET.into(),
        BROWSER_ORIGIN,
    )
    .expect("OIDC configuration");
    let authentication = ServerOidcAuthenticationUseCases::new(
        database,
        configuration,
        reqwest::Client::new(),
        None,
    );

    assert!(authentication.begin_login().await.is_ok());
}
