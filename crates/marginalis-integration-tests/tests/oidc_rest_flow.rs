//! OIDC login から REST ノート操作までを、実HTTPのDiscovery・code交換を含めて
//! 一気通貫で検証する。docs/roadmap.md 段階1・Issue 030 のシナリオ1と2に対応する。

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, Response, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use marginalis_application::RootInitializationService;
use marginalis_auth_oidc::{OidcAuthentication, OidcConfiguration};
use marginalis_files::FileNoteStore;
use marginalis_integration_tests::MockIdentityProvider;
use marginalis_mcp::McpTools;
use marginalis_server::{
    ServerMcpAuthenticator, ServerMcpOAuthService, ServerNoteUseCases,
    ServerWebAuthenticationUseCases, SystemClock, SystemRandom,
};
use marginalis_sqlite::SqliteDatabase;
use marginalis_web::{ApiState, McpEndpoint, McpRateLimiter, router};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use url::Url;

const BROWSER_ORIGIN: &str = "https://marginalis.example.test";
const CLIENT_ID: &str = "marginalis";
const CLIENT_SECRET: &str = "integration-client-secret";
const ROOT_PASSWORD: &str = "integration-root-password";
const MCP_RESOURCE: &str = "https://marginalis.example.test/mcp";
const MCP_CLIENT_ID: &str = "integration-mcp-client";
const MCP_CALLBACK: &str = "http://127.0.0.1:4567/callback";

struct McpTokens {
    access_token: String,
    refresh_token: String,
}

/// mock IdPと空のdataDirへ接続した、試験ごとに独立するアプリケーション一式。
struct TestServer {
    idp: MockIdentityProvider,
    app: Router,
    directory: PathBuf,
}

impl TestServer {
    /// approval policyの新規SQLiteとroot credentialを持つサーバーを組み立てる。
    async fn start() -> Self {
        let idp = MockIdentityProvider::start(CLIENT_ID, CLIENT_SECRET).await;
        let directory =
            std::env::temp_dir().join(format!("marginalis-integration-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&directory).expect("test directory");
        let database = SqliteDatabase::connect(&format!(
            "sqlite:{}",
            directory.join("marginalis.sqlite").display()
        ))
        .await
        .expect("database");
        let root_store = database.root_credential_store();
        RootInitializationService::new(&root_store, &SystemRandom, &SystemClock)
            .initialize_if_missing(ROOT_PASSWORD.into())
            .await
            .expect("root initialization");
        let configuration = OidcConfiguration::new(
            idp.issuer.clone(),
            CLIENT_ID.into(),
            CLIENT_SECRET.into(),
            BROWSER_ORIGIN,
        )
        .expect("OIDC configuration");
        let oidc = OidcAuthentication::discover(&configuration)
            .await
            .expect("discovery against the mock IdP");
        let authentication = Arc::new(ServerWebAuthenticationUseCases::with_oidc(
            database.clone(),
            oidc,
        ));
        let notes = Arc::new(ServerNoteUseCases::new(
            database.clone(),
            FileNoteStore::open(&directory).expect("note sources"),
        ));
        let oauth = Arc::new(ServerMcpOAuthService::new(database.clone(), Vec::new()));
        let state = ApiState::new(
            notes.clone(),
            authentication.clone(),
            authentication.clone(),
            authentication,
            BROWSER_ORIGIN.into(),
        )
        .with_mcp(McpEndpoint {
            tools: McpTools::new(notes),
            authenticator: Arc::new(ServerMcpAuthenticator::new(database, MCP_RESOURCE.into())),
            oauth: oauth.clone(),
            oauth_administration: oauth,
            resource_uri: MCP_RESOURCE.into(),
            metadata_uri: format!("{BROWSER_ORIGIN}/.well-known/oauth-protected-resource/mcp"),
            authorization_server_uri: BROWSER_ORIGIN.into(),
            authorization_endpoint_uri: format!("{BROWSER_ORIGIN}/oauth/authorize"),
            token_endpoint_uri: format!("{BROWSER_ORIGIN}/oauth/token"),
            allowed_origin: BROWSER_ORIGIN.into(),
            rate_limiter: McpRateLimiter::new(120),
        });
        let app = router(state);
        Self {
            idp,
            app,
            directory,
        }
    }

    fn finish(self) {
        std::fs::remove_dir_all(self.directory).expect("remove test directory");
    }
}

async fn send(app: &Router, request: Request<Body>) -> Response<Body> {
    app.clone().oneshot(request).await.expect("HTTP response")
}

fn set_cookie_value(response: &Response<Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|value| {
            let value = value.to_str().ok()?;
            let cookie = value.split(';').next().unwrap_or(value);
            let (key, value) = cookie.split_once('=')?;
            (key == name && !value.is_empty()).then(|| value.to_owned())
        })
}

async fn json_body(response: Response<Body>) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

async fn text_body(response: Response<Body>) -> String {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    String::from_utf8(bytes.to_vec()).expect("UTF-8 body")
}

/// `/auth/oidc/login`を実行し、IdPへ渡る認可要求のstate・nonce・code challengeを返す。
async fn begin_login(app: &Router) -> (String, String, String) {
    let response = send(
        app,
        Request::builder()
            .uri("/auth/oidc/login")
            .body(Body::empty())
            .expect("login request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    let destination = response
        .headers()
        .get(header::LOCATION)
        .expect("authorization request URL")
        .to_str()
        .expect("location header");
    let destination = Url::parse(destination).expect("authorization request URL");
    let query: HashMap<String, String> = destination
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    assert_eq!(
        query.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    let state = query.get("state").expect("state parameter").clone();
    assert_eq!(
        set_cookie_value(&response, "marginalis_oidc_state").as_deref(),
        Some(state.as_str())
    );
    (
        state,
        query.get("nonce").expect("nonce parameter").clone(),
        query
            .get("code_challenge")
            .expect("code challenge parameter")
            .clone(),
    )
}

async fn complete_login(app: &Router, state: &str, code: &str) -> Response<Body> {
    send(
        app,
        Request::builder()
            .uri(format!("/auth/oidc/callback?code={code}&state={state}"))
            .header(header::COOKIE, format!("marginalis_oidc_state={state}"))
            .body(Body::empty())
            .expect("callback request"),
    )
    .await
}

/// root loginを実行し、sessionとCSRF tokenを返す。
async fn root_login(app: &Router) -> (String, String) {
    let mut request = Request::builder()
        .method("POST")
        .uri("/auth/root/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({ "password": ROOT_PASSWORD }).to_string(),
        ))
        .expect("root login request");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40001))));
    let response = send(app, request).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    (
        set_cookie_value(&response, "marginalis_session").expect("root session"),
        set_cookie_value(&response, "marginalis_csrf").expect("root CSRF token"),
    )
}

/// rootとして登録policyを`open`へ変更する。
async fn set_registration_policy_open(app: &Router, session: &str, csrf: &str) {
    let response = send(
        app,
        Request::builder()
            .method("PUT")
            .uri("/api/v1/admin/registration-policy")
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .header("x-csrf-token", csrf)
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"policy":"open"}"#))
            .expect("policy update request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// activeなOIDCユーザーとしてloginし、sessionとCSRF tokenを返す。
async fn login_active_user(server: &TestServer, subject: &str, code: &str) -> (String, String) {
    let (state, nonce, challenge) = begin_login(&server.app).await;
    server.idp.approve(code, subject, &nonce, &challenge);
    let response = complete_login(&server.app, &state, code).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    (
        set_cookie_value(&response, "marginalis_session").expect("user session"),
        set_cookie_value(&response, "marginalis_csrf").expect("user CSRF token"),
    )
}

fn note_source(token: &str) -> String {
    format!(
        "= Integration note\n\
         :note-id: 01800000-0000-7000-8000-000000000001\n\
         :creator-id: 01800000-0000-7000-8000-000000000002\n\
         :created-at: 2026-01-01T00:00:00.000Z\n\
         :updated-at: 2026-01-01T00:00:00.000Z\n\
         :tags: integration\n\n\
         {token} body.\n"
    )
}

/// ノートを作成し、正本のURLを返す。
async fn create_note(app: &Router, session: &str, csrf: &str, source: String) -> String {
    let response = send(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/notes")
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .header("x-csrf-token", csrf)
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Body::from(source))
            .expect("note creation request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    response
        .headers()
        .get(header::LOCATION)
        .expect("created note location")
        .to_str()
        .expect("location header")
        .to_owned()
}

/// 正本と現在の`ETag`を取得する。
async fn read_note(app: &Router, session: &str, location: &str) -> (String, String) {
    let response = send(
        app,
        Request::builder()
            .uri(location)
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .body(Body::empty())
            .expect("note source request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response
        .headers()
        .get(header::ETAG)
        .expect("note ETag")
        .to_str()
        .expect("ETag header")
        .to_owned();
    (text_body(response).await, etag)
}

async fn search(app: &Router, session: &str, query: &str) -> serde_json::Value {
    let response = send(
        app,
        Request::builder()
            .uri(format!("/api/v1/search?q={query}"))
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .body(Body::empty())
            .expect("search request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

#[tokio::test]
async fn oidc_approval_flow_reaches_rest_notes() {
    let server = TestServer::start().await;

    // 1. 初回OIDC login。approval policyでは保留ユーザーだけが作られ、sessionは発行されない。
    let (state, nonce, challenge) = begin_login(&server.app).await;
    server
        .idp
        .approve("code-1", "subject-1", &nonce, &challenge);
    let response = complete_login(&server.app, &state, "code-1").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(set_cookie_value(&response, "marginalis_session").is_none());

    // 2. rootがpendingユーザーを確認して有効化する。
    let (root_session, root_csrf) = root_login(&server.app).await;
    let response = send(
        &server.app,
        Request::builder()
            .uri("/api/v1/admin/users/pending")
            .header(header::COOKIE, format!("marginalis_session={root_session}"))
            .body(Body::empty())
            .expect("pending list request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let pending = json_body(response).await;
    let pending = pending.as_array().expect("pending user array");
    assert_eq!(pending.len(), 1);
    let pending_user_id = pending[0]["user_id"].as_str().expect("user ID").to_owned();

    let response = send(
        &server.app,
        Request::builder()
            .method("PUT")
            .uri(format!("/api/v1/admin/users/{pending_user_id}/activate"))
            .header(header::COOKIE, format!("marginalis_session={root_session}"))
            .header("x-csrf-token", &root_csrf)
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .body(Body::empty())
            .expect("activation request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // 3. 二回目のOIDC loginで通常sessionを取得する。stateは一回限りであることも確認する。
    let (state, nonce, challenge) = begin_login(&server.app).await;
    server
        .idp
        .approve("code-2", "subject-1", &nonce, &challenge);
    let response = complete_login(&server.app, &state, "code-2").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let session = set_cookie_value(&response, "marginalis_session").expect("user session");
    let csrf = set_cookie_value(&response, "marginalis_csrf").expect("user CSRF token");
    let replayed = complete_login(&server.app, &state, "code-2").await;
    assert_eq!(replayed.status(), StatusCode::UNAUTHORIZED);

    let response = send(
        &server.app,
        Request::builder()
            .uri("/api/v1/session")
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .body(Body::empty())
            .expect("session request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let current = json_body(response).await;
    assert_eq!(current["user_id"].as_str(), Some(pending_user_id.as_str()));
    assert_eq!(current["is_root"].as_bool(), Some(false));

    // 4. 取得したsessionでRESTノートを作成・取得・検索する。保護属性はserver値になる。
    let location = create_note(
        &server.app,
        &session,
        &csrf,
        note_source("integratione2etoken"),
    )
    .await;
    let (stored, _etag) = read_note(&server.app, &session, &location).await;
    assert!(stored.contains("integratione2etoken"));
    assert!(!stored.contains(":note-id: 01800000-0000-7000-8000-000000000001"));

    let results = search(&server.app, &session, "integratione2etoken").await;
    let notes = results["notes"].as_array().expect("search results");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["title"].as_str(), Some("Integration note"));

    server.finish();
}

#[tokio::test]
async fn revision_conflict_and_two_phase_deletion_over_http() {
    let server = TestServer::start().await;
    let (root_session, root_csrf) = root_login(&server.app).await;
    set_registration_policy_open(&server.app, &root_session, &root_csrf).await;
    let (session, csrf) = login_active_user(&server, "subject-editor", "code-editor").await;

    let location = create_note(&server.app, &session, &csrf, note_source("conflicttoken")).await;
    let (stored, etag) = read_note(&server.app, &session, &location).await;

    // 正しい`If-Match`の更新は成功する。
    let updated_source = stored.replace("conflicttoken body.", "conflicttoken revised body.");
    let response = send(
        &server.app,
        Request::builder()
            .method("PUT")
            .uri(&location)
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .header("x-csrf-token", &csrf)
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::IF_MATCH, &etag)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Body::from(updated_source.clone()))
            .expect("update request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // 古い`ETag`の再更新は競合になる。
    let response = send(
        &server.app,
        Request::builder()
            .method("PUT")
            .uri(&location)
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .header("x-csrf-token", &csrf)
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::IF_MATCH, &etag)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Body::from(updated_source))
            .expect("stale update request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await["code"].as_str(), Some("conflict"));

    // 最新revisionで削除準備し、確認tokenで確定する。
    let (_, latest_etag) = read_note(&server.app, &session, &location).await;
    let note_id = location
        .strip_prefix("/api/v1/notes/")
        .and_then(|rest| rest.strip_suffix("/source"))
        .expect("note ID in location");
    let response = send(
        &server.app,
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/notes/{note_id}/delete-preparations"))
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .header("x-csrf-token", &csrf)
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::IF_MATCH, &latest_etag)
            .body(Body::empty())
            .expect("delete preparation request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let preparation = json_body(response).await;
    assert_eq!(preparation["incoming_reference_count"].as_u64(), Some(0));
    let confirmation_token = preparation["confirmation_token"]
        .as_str()
        .expect("confirmation token")
        .to_owned();

    let response = send(
        &server.app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/notes/delete-confirmations")
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .header("x-csrf-token", &csrf)
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "confirmation_token": confirmation_token }).to_string(),
            ))
            .expect("delete confirmation request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // 削除後は取得が404、検索は空になる。
    let response = send(
        &server.app,
        Request::builder()
            .uri(&location)
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .body(Body::empty())
            .expect("deleted note request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let results = search(&server.app, &session, "conflicttoken").await;
    assert_eq!(
        results["notes"].as_array().map(Vec::len),
        Some(0),
        "deleted note must not appear in search"
    );

    server.finish();
}

#[tokio::test]
async fn other_users_cannot_observe_private_notes() {
    let server = TestServer::start().await;
    let (root_session, root_csrf) = root_login(&server.app).await;
    set_registration_policy_open(&server.app, &root_session, &root_csrf).await;
    let (owner_session, owner_csrf) = login_active_user(&server, "subject-owner", "code-a").await;
    let (other_session, _) = login_active_user(&server, "subject-other", "code-b").await;

    let location = create_note(
        &server.app,
        &owner_session,
        &owner_csrf,
        note_source("privatetoken"),
    )
    .await;

    // 所有者には見え、他ユーザーには存在自体を示さない。
    let (stored, _) = read_note(&server.app, &owner_session, &location).await;
    assert!(stored.contains("privatetoken"));
    let response = send(
        &server.app,
        Request::builder()
            .uri(&location)
            .header(
                header::COOKIE,
                format!("marginalis_session={other_session}"),
            )
            .body(Body::empty())
            .expect("other user note request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        json_body(response).await["code"].as_str(),
        Some("not-found")
    );

    let owner_results = search(&server.app, &owner_session, "privatetoken").await;
    assert_eq!(
        owner_results["notes"].as_array().map(Vec::len),
        Some(1),
        "owner search must find the note"
    );
    let other_results = search(&server.app, &other_session, "privatetoken").await;
    assert_eq!(
        other_results["notes"].as_array().map(Vec::len),
        Some(0),
        "other user search must not reveal the note"
    );

    server.finish();
}

/// rootとしてMCP public clientを事前登録する。
async fn register_mcp_client(app: &Router, session: &str, csrf: &str) {
    let response = send(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/admin/mcp-clients")
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .header("x-csrf-token", csrf)
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "client_id": MCP_CLIENT_ID,
                    "display_name": "Integration MCP client",
                    "redirect_uris": [MCP_CALLBACK],
                })
                .to_string(),
            ))
            .expect("client registration request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// Authorization Code + PKCEをHTTPで通し、`notes:read`のtoken pairを得る。
async fn mcp_tokens(app: &Router, session: &str, csrf: &str, verifier: &str) -> McpTokens {
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("response_type", "code")
        .append_pair("client_id", MCP_CLIENT_ID)
        .append_pair("redirect_uri", MCP_CALLBACK)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("scope", "notes:read")
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .finish();
    let cookies = format!("marginalis_session={session}; marginalis_csrf={csrf}");
    let response = send(
        app,
        Request::builder()
            .uri(format!("/oauth/authorize?{query}"))
            .header(header::COOKIE, &cookies)
            .body(Body::empty())
            .expect("authorization request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", MCP_CLIENT_ID)
        .append_pair("redirect_uri", MCP_CALLBACK)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("scope", "notes:read")
        .append_pair("code_challenge", &challenge)
        .append_pair("csrf_token", csrf)
        .append_pair("decision", "approve")
        .finish();
    let response = send(
        app,
        Request::builder()
            .method("POST")
            .uri("/oauth/authorize")
            .header(header::COOKIE, &cookies)
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(form))
            .expect("approval request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response
        .headers()
        .get(header::LOCATION)
        .expect("authorization redirect")
        .to_str()
        .expect("location header");
    let code = Url::parse(location)
        .expect("redirect URL")
        .query_pairs()
        .find_map(|(key, value)| (key == "code").then(|| value.into_owned()))
        .expect("authorization code");

    let token_form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", &code)
        .append_pair("client_id", MCP_CLIENT_ID)
        .append_pair("redirect_uri", MCP_CALLBACK)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("code_verifier", verifier)
        .finish();
    let response = send(
        app,
        Request::builder()
            .method("POST")
            .uri("/oauth/token")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(token_form))
            .expect("token request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    McpTokens {
        access_token: body["access_token"]
            .as_str()
            .expect("access token")
            .to_owned(),
        refresh_token: body["refresh_token"]
            .as_str()
            .expect("refresh token")
            .to_owned(),
    }
}

/// MCPの`search_notes`を実行し、`structuredContent.notes`を返す。
async fn mcp_search(app: &Router, access_token: &str, query: &str) -> serde_json::Value {
    let response = send(
        app,
        Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
            .body(Body::from(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/call",
                    "params": { "name": "search_notes", "arguments": { "query": query } },
                })
                .to_string(),
            ))
            .expect("MCP request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["result"]["structuredContent"]["notes"].clone()
}

#[tokio::test]
async fn mcp_search_visibility_matches_rest() {
    let server = TestServer::start().await;
    let (root_session, root_csrf) = root_login(&server.app).await;
    set_registration_policy_open(&server.app, &root_session, &root_csrf).await;
    register_mcp_client(&server.app, &root_session, &root_csrf).await;
    let (owner_session, owner_csrf) =
        login_active_user(&server, "subject-mcp-owner", "code-m1").await;
    let (other_session, other_csrf) =
        login_active_user(&server, "subject-mcp-other", "code-m2").await;

    create_note(
        &server.app,
        &owner_session,
        &owner_csrf,
        note_source("mcpparitytoken"),
    )
    .await;

    let owner_tokens = mcp_tokens(
        &server.app,
        &owner_session,
        &owner_csrf,
        "integration-pkce-verifier-owner-0123456789",
    )
    .await;
    let other_tokens = mcp_tokens(
        &server.app,
        &other_session,
        &other_csrf,
        "integration-pkce-verifier-other-0123456789",
    )
    .await;

    // RESTとMCPで、所有者には見え、他ユーザーには存在が漏れない。
    let owner_rest = search(&server.app, &owner_session, "mcpparitytoken").await;
    assert_eq!(owner_rest["notes"].as_array().map(Vec::len), Some(1));
    let other_rest = search(&server.app, &other_session, "mcpparitytoken").await;
    assert_eq!(other_rest["notes"].as_array().map(Vec::len), Some(0));

    let owner_mcp = mcp_search(&server.app, &owner_tokens.access_token, "mcpparitytoken").await;
    let owner_mcp = owner_mcp.as_array().expect("owner MCP results");
    assert_eq!(owner_mcp.len(), 1);
    assert_eq!(
        owner_mcp[0]["title"].as_str(),
        Some("Integration note"),
        "MCP search must return the owner's note"
    );
    let other_mcp = mcp_search(&server.app, &other_tokens.access_token, "mcpparitytoken").await;
    assert_eq!(
        other_mcp.as_array().map(Vec::len),
        Some(0),
        "MCP search must not reveal another user's note"
    );

    server.finish();
}

#[tokio::test]
async fn revoking_mcp_authorization_invalidates_issued_tokens() {
    let server = TestServer::start().await;
    let (root_session, root_csrf) = root_login(&server.app).await;
    set_registration_policy_open(&server.app, &root_session, &root_csrf).await;
    register_mcp_client(&server.app, &root_session, &root_csrf).await;
    let (session, csrf) =
        login_active_user(&server, "subject-mcp-revocation", "code-mcp-revocation").await;
    let tokens = mcp_tokens(
        &server.app,
        &session,
        &csrf,
        "integration-pkce-verifier-revocation-0123456789",
    )
    .await;

    let response = send(
        &server.app,
        Request::builder()
            .method("DELETE")
            .uri(format!(
                "/api/v1/mcp-authorizations?client_id={MCP_CLIENT_ID}"
            ))
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .header("x-csrf-token", &csrf)
            .header(header::ORIGIN, BROWSER_ORIGIN)
            .header("sec-fetch-site", "same-origin")
            .body(Body::empty())
            .expect("authorization revocation request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &server.app,
        Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::CONTENT_TYPE, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", tokens.access_token),
            )
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            ))
            .expect("MCP request with revoked access token"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let refresh_form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("client_id", MCP_CLIENT_ID)
        .append_pair("resource", MCP_RESOURCE)
        .append_pair("refresh_token", &tokens.refresh_token)
        .finish();
    let response = send(
        &server.app,
        Request::builder()
            .method("POST")
            .uri("/oauth/token")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(refresh_form))
            .expect("refresh request with revoked token"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json_body(response).await["code"].as_str(),
        Some("validation-failed")
    );

    server.finish();
}

#[tokio::test]
async fn cookie_mutations_require_csrf_origin_and_validate_fetch_metadata_when_present() {
    let server = TestServer::start().await;
    let (root_session, root_csrf) = root_login(&server.app).await;
    set_registration_policy_open(&server.app, &root_session, &root_csrf).await;
    let (session, csrf) = login_active_user(&server, "subject-csrf", "code-csrf").await;

    let request = |csrf_header: Option<&str>, origin: &str, fetch_site: Option<&str>| {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/api/v1/notes")
            .header(header::COOKIE, format!("marginalis_session={session}"))
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8");
        if let Some(fetch_site) = fetch_site {
            builder = builder.header("sec-fetch-site", fetch_site);
        }
        if let Some(token) = csrf_header {
            builder = builder.header("x-csrf-token", token);
        }
        builder
            .body(Body::from(note_source("csrfguardtoken")))
            .expect("note creation request")
    };

    // CSRF tokenの欠落、Originの不一致、明示されたcross-siteは拒否される。
    let response = send(
        &server.app,
        request(None, BROWSER_ORIGIN, Some("same-origin")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = send(
        &server.app,
        request(
            Some(&csrf),
            "https://evil.example.test",
            Some("same-origin"),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = send(
        &server.app,
        request(Some(&csrf), BROWSER_ORIGIN, Some("cross-site")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = send(
        &server.app,
        request(Some("wrong-token"), BROWSER_ORIGIN, Some("same-origin")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // OriginとCSRF tokenが正しければ、Fetch Metadata非対応のclientも作成できる。
    let response = send(&server.app, request(Some(&csrf), BROWSER_ORIGIN, None)).await;
    assert_eq!(response.status(), StatusCode::CREATED);

    // 許可されたFetch Metadataが明示された場合も作成できる。
    let response = send(
        &server.app,
        request(Some(&csrf), BROWSER_ORIGIN, Some("same-origin")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);

    server.finish();
}
