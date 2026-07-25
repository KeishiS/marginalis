//! v0.3.0専用のHTTP APIと早期閲覧UI。
//!
//! 旧公開API・ローカル管理者・ローカル`UserId`を参照しない。composition rootは
//! v0.3.0ではこのrouterだけを公開する。

mod auth;
mod error;
mod html;
mod mcp_transport;
mod notes;
mod oauth;
mod security;
mod state;
mod ui;

pub use state::{ApiState, McpEndpoint};

use super::{RequestId, assign_request_id};
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::{StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::{Level, info_span};

use self::{
    auth::{begin_login, complete_login, logout},
    error::{HandlerResult, problem},
    mcp_transport::mcp_post,
    notes::{
        create_note, delete_note, export_note, list_notes, read_note, restore_note, session,
        update_note,
    },
    oauth::{
        mcp_authorize, mcp_authorize_consent, mcp_authorize_post, mcp_register_client,
        mcp_resource_metadata, mcp_server_metadata, mcp_token, revoke_mcp_authorization,
    },
    security::security_headers,
    ui::{home, view_note},
};

pub const API_VERSION: &str = "v2";
pub const OPENAPI_DOCUMENT: &str = include_str!("../../../docs/openapi.json");
pub fn router(state: ApiState) -> Router {
    let mut router = Router::new()
        .route("/", get(home))
        .route("/notes/{note_id}", get(view_note))
        .route("/api/v2/openapi.json", get(openapi))
        .route("/auth/oidc/login", get(begin_login))
        .route("/auth/oidc/callback", get(complete_login))
        .route("/auth/logout", post(logout))
        .route(
            "/oauth/authorize",
            get(mcp_authorize)
                .post(mcp_authorize_post)
                .layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/oauth/authorize/consent",
            post(mcp_authorize_consent).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/oauth/register",
            post(mcp_register_client).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/oauth/token",
            post(mcp_token).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/mcp",
            post(mcp_post).layer(DefaultBodyLimit::max(1024 * 1024)),
        )
        .route("/api/v2/health", get(health))
        .route("/api/v2/session", get(session))
        .route("/api/v2/notes", get(list_notes).post(create_note))
        .route(
            "/api/v2/notes/{note_id}",
            get(read_note).put(update_note).delete(delete_note),
        )
        .route("/api/v2/notes/{note_id}/restore", post(restore_note))
        .route("/api/v2/notes/{note_id}/source", get(export_note))
        .route(
            "/api/v2/mcp-authorizations/{client_id}",
            axum::routing::delete(revoke_mcp_authorization),
        );
    if let Some(endpoint) = state.mcp.as_ref() {
        let resource_metadata_path = url::Url::parse(&endpoint.metadata_uri)
            .expect("validated MCP resource metadata URL")
            .path()
            .to_owned();
        let server_metadata_path = url::Url::parse(&endpoint.authorization_server_metadata_uri)
            .expect("validated authorization server metadata URL")
            .path()
            .to_owned();
        router = router
            .route(&resource_metadata_path, get(mcp_resource_metadata))
            .route(&server_metadata_path, get(mcp_server_metadata));
    }
    router
        .with_state(state)
        // Axum applies the last layer first. Assign the ID before TraceLayer creates its span.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    let request_id = request
                        .extensions()
                        .get::<RequestId>()
                        .map(|id| id.0.as_str())
                        .unwrap_or("missing");
                    info_span!(
                        "http_request",
                        request_id,
                        method = %request.method(),
                        path = request.uri().path(),
                    )
                })
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(assign_request_id))
}

async fn openapi() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        OPENAPI_DOCUMENT,
    )
        .into_response()
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "api_version": API_VERSION}))
}

fn mcp_endpoint(state: &ApiState) -> HandlerResult<&Arc<McpEndpoint>> {
    state
        .mcp
        .as_ref()
        .ok_or_else(|| problem(StatusCode::NOT_FOUND, "not_found", "MCP is not available"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        body::Body,
        http::{HeaderMap, Request},
    };
    use marginalis_application::{
        AuthenticationUseCaseError, McpAuthorizationClient, McpOAuthUseCaseError, McpOAuthUseCases,
        McpTokenPair, McpValidatedAuthorizationRequest, NoteUseCaseError, NoteUseCases,
        OidcAuthenticationUseCases, WebSessionUseCases,
    };
    use marginalis_domain::{
        Actor, AuthenticatedSession, McpAuthenticatedActor, McpOAuthClient, Note, NoteDraft,
        NoteId, UnixMillis, WebSession,
    };
    use std::time::{Duration, Instant};
    use tower::ServiceExt;

    use super::{
        auth::{
            RETURN_TO_COOKIE, authenticated_form_actor, external_path, valid_return_to,
            validate_mutation_origin,
        },
        state::McpRegistrationRateLimiter,
    };

    struct Notes;

    #[async_trait]
    impl NoteUseCases for Notes {
        async fn list_visible_notes(&self, _actor: Actor) -> Result<Vec<Note>, NoteUseCaseError> {
            Ok(Vec::new())
        }

        async fn read_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
        ) -> Result<Note, NoteUseCaseError> {
            Err(NoteUseCaseError::NotFound)
        }

        async fn create_note(
            &self,
            _actor: Actor,
            _draft: NoteDraft,
        ) -> Result<Note, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn update_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _draft: NoteDraft,
            _expected_revision: i64,
        ) -> Result<Note, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn soft_delete_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _expected_revision: i64,
        ) -> Result<Note, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        async fn restore_note(
            &self,
            _actor: Actor,
            _note_id: NoteId,
            _expected_revision: i64,
        ) -> Result<Note, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        fn export_note_source(&self, _note: &Note) -> Result<String, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }

        fn render_note_html(&self, _note: &Note) -> Result<String, NoteUseCaseError> {
            Err(NoteUseCaseError::Unavailable)
        }
    }

    struct Sessions;

    #[async_trait]
    impl WebSessionUseCases for Sessions {
        async fn authenticate_session(
            &self,
            _session_id: String,
        ) -> Result<Option<AuthenticatedSession>, AuthenticationUseCaseError> {
            Ok(None)
        }

        async fn verify_csrf(
            &self,
            _session_id: String,
            _csrf_token: String,
        ) -> Result<bool, AuthenticationUseCaseError> {
            Ok(false)
        }

        async fn issue_session(
            &self,
            _actor: Actor,
        ) -> Result<WebSession, AuthenticationUseCaseError> {
            Err(AuthenticationUseCaseError::Unavailable)
        }

        async fn revoke_session(
            &self,
            _session_id: String,
        ) -> Result<(), AuthenticationUseCaseError> {
            Ok(())
        }
    }

    struct ActiveSessions;

    #[async_trait]
    impl WebSessionUseCases for ActiveSessions {
        async fn authenticate_session(
            &self,
            session_id: String,
        ) -> Result<Option<AuthenticatedSession>, AuthenticationUseCaseError> {
            Ok(
                (session_id == "active-session").then(|| AuthenticatedSession {
                    actor: Actor {
                        issuer: "https://id.example.test".into(),
                        subject: "alice".into(),
                        is_administrator: false,
                    },
                    idle_expires_at: UnixMillis::new(i64::MAX - 1),
                    absolute_expires_at: UnixMillis::new(i64::MAX),
                }),
            )
        }

        async fn verify_csrf(
            &self,
            session_id: String,
            csrf_token: String,
        ) -> Result<bool, AuthenticationUseCaseError> {
            Ok(session_id == "active-session" && csrf_token == "session-csrf")
        }

        async fn issue_session(
            &self,
            _actor: Actor,
        ) -> Result<WebSession, AuthenticationUseCaseError> {
            Err(AuthenticationUseCaseError::Unavailable)
        }

        async fn revoke_session(
            &self,
            _session_id: String,
        ) -> Result<(), AuthenticationUseCaseError> {
            Ok(())
        }
    }

    struct Oidc;

    #[async_trait]
    impl OidcAuthenticationUseCases for Oidc {
        async fn begin_login(&self) -> Result<String, AuthenticationUseCaseError> {
            Ok("https://id.example.test/authorize".into())
        }

        async fn complete_login(
            &self,
            _code: String,
            _state: String,
        ) -> Result<Actor, AuthenticationUseCaseError> {
            Err(AuthenticationUseCaseError::Unavailable)
        }
    }

    struct Mcp;
    #[async_trait]
    impl McpOAuthUseCases for Mcp {
        async fn register_client(
            &self,
            client: McpOAuthClient,
        ) -> Result<(), McpOAuthUseCaseError> {
            if client
                .redirect_uris
                .iter()
                .any(|uri| uri.starts_with("http://remote.example"))
            {
                Err(McpOAuthUseCaseError::InvalidRedirectUri)
            } else {
                Ok(())
            }
        }
        async fn resolve_authorization_client(
            &self,
            client_id: String,
            redirect_uri: Option<String>,
        ) -> Result<McpAuthorizationClient, McpOAuthUseCaseError> {
            let redirect_uri =
                redirect_uri.unwrap_or_else(|| "https://client.example.test/callback".into());
            Ok(McpAuthorizationClient {
                client: McpOAuthClient {
                    client_id,
                    display_name: "Test MCP client".into(),
                    redirect_uris: vec![redirect_uri.clone()],
                },
                redirect_uri,
            })
        }
        async fn validate_authorization_request(
            &self,
            request: marginalis_application::McpAuthorizationRequest,
        ) -> Result<McpValidatedAuthorizationRequest, McpOAuthUseCaseError> {
            if request.resource_uri != "https://example.test/mcp" {
                return Err(McpOAuthUseCaseError::InvalidTarget);
            }
            let redirect_uri = request
                .redirect_uri
                .unwrap_or_else(|| "https://client.example.test/callback".into());
            Ok(McpValidatedAuthorizationRequest {
                client: McpOAuthClient {
                    client_id: request.client_id,
                    display_name: "Test MCP client".into(),
                    redirect_uris: vec![redirect_uri.clone()],
                },
                redirect_uri,
                resource_uri: request.resource_uri,
                scopes: request.scopes,
                code_challenge: request.code_challenge,
            })
        }
        async fn authorize(
            &self,
            _actor: Actor,
            _request: McpValidatedAuthorizationRequest,
        ) -> Result<String, McpOAuthUseCaseError> {
            Err(McpOAuthUseCaseError::InvalidRequest)
        }
        async fn exchange_authorization_code(
            &self,
            _code: String,
            _client_id: String,
            _redirect_uri: Option<String>,
            _resource_uri: String,
            _verifier: String,
        ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
            Ok(McpTokenPair {
                access_token: "access-token".into(),
                refresh_token: "refresh-token".into(),
                access_expires_in_seconds: 300,
                scope: "notes:read".into(),
            })
        }
        async fn refresh_access_token(
            &self,
            _refresh_token: String,
            _client_id: String,
            _resource_uri: String,
            _scopes: Option<Vec<String>>,
        ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
            Err(McpOAuthUseCaseError::InvalidGrant)
        }
        async fn authenticate(
            &self,
            token: String,
            _resource_uri: String,
        ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError> {
            Ok(
                matches!(token.as_str(), "valid-token" | "read-token").then(|| {
                    McpAuthenticatedActor {
                        actor: Actor {
                            issuer: "https://kanidm.example.test".into(),
                            subject: "alice".into(),
                            is_administrator: false,
                        },
                        scopes: if token == "read-token" {
                            vec!["notes:read".into()]
                        } else {
                            vec![
                                "notes:read".into(),
                                "notes:write".into(),
                                "notes:delete".into(),
                            ]
                        },
                    }
                }),
            )
        }
        async fn revoke(
            &self,
            _actor: Actor,
            _client_id: String,
        ) -> Result<(), McpOAuthUseCaseError> {
            Ok(())
        }
    }

    fn app() -> Router {
        router(ApiState::new(
            Arc::new(Notes),
            Arc::new(Sessions),
            Arc::new(Oidc),
            "/".into(),
            "https://example.test".into(),
        ))
    }

    fn mcp_app() -> Router {
        router(
            ApiState::new(
                Arc::new(Notes),
                Arc::new(Sessions),
                Arc::new(Oidc),
                "/".into(),
                "https://example.test".into(),
            )
            .with_mcp(McpEndpoint {
                oauth: Arc::new(Mcp),
                notes: Arc::new(Notes),
                allowed_origins: vec!["https://chatgpt.com".into()],
                resource_uri: "https://example.test/mcp".into(),
                metadata_uri: "https://example.test/.well-known/oauth-protected-resource/mcp"
                    .into(),
                authorization_server_uri: "https://example.test".into(),
                authorization_server_metadata_uri:
                    "https://example.test/.well-known/oauth-authorization-server".into(),
                authorization_endpoint_uri: "https://example.test/oauth/authorize".into(),
                token_endpoint_uri: "https://example.test/oauth/token".into(),
            }),
        )
    }

    fn subpath_mcp_app() -> Router {
        router(
            ApiState::new(
                Arc::new(Notes),
                Arc::new(Sessions),
                Arc::new(Oidc),
                "/marginalis".into(),
                "https://example.test".into(),
            )
            .with_mcp(McpEndpoint {
                oauth: Arc::new(Mcp),
                notes: Arc::new(Notes),
                allowed_origins: vec![],
                resource_uri: "https://example.test/marginalis/mcp".into(),
                metadata_uri:
                    "https://example.test/.well-known/oauth-protected-resource/marginalis/mcp"
                        .into(),
                authorization_server_uri: "https://example.test/marginalis".into(),
                authorization_server_metadata_uri:
                    "https://example.test/.well-known/oauth-authorization-server/marginalis".into(),
                authorization_endpoint_uri: "https://example.test/marginalis/oauth/authorize"
                    .into(),
                token_endpoint_uri: "https://example.test/marginalis/oauth/token".into(),
            }),
        )
    }

    #[test]
    fn external_paths_preserve_the_configured_subpath() {
        assert_eq!(external_path("/", "/notes/123"), "/notes/123");
        assert!(valid_return_to("/oauth/authorize?client_id=client", "/"));
        assert_eq!(
            external_path("/marginalis", "/notes/123"),
            "/marginalis/notes/123"
        );
        assert!(valid_return_to(
            "/marginalis/oauth/authorize?client_id=client",
            "/marginalis"
        ));
        assert!(!valid_return_to(
            "/oauth/authorize?client_id=client",
            "/marginalis"
        ));
        assert!(!valid_return_to("//oauth/authorize?client_id=client", "/"));
        assert!(!valid_return_to(
            "/oauth/authorize?client_id=client\r\nLocation:%20https://evil.test",
            "/"
        ));
    }

    #[tokio::test]
    async fn health_is_public_but_notes_require_a_session() {
        let health = app()
            .oneshot(
                Request::get("/api/v2/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(health.status(), StatusCode::OK);
        let request_id = health
            .headers()
            .get(super::super::REQUEST_ID_HEADER)
            .expect("request ID")
            .to_str()
            .expect("request ID value");
        assert_eq!(
            uuid::Uuid::parse_str(request_id)
                .expect("UUID request ID")
                .get_version_num(),
            7
        );
        assert_eq!(
            health.headers().get(header::CACHE_CONTROL),
            Some(&"no-store".parse().expect("header"))
        );
        let notes = app()
            .oneshot(
                Request::get("/api/v2/notes")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(notes.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn openapi_is_served_from_the_embedded_contract() {
        let response = app()
            .oneshot(
                Request::get("/api/v2/openapi.json")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(OPENAPI_DOCUMENT, include_str!("../../../docs/openapi.json"));
    }

    #[tokio::test]
    async fn mcp_metadata_is_available_when_enabled() {
        let response = mcp_app()
            .oneshot(
                Request::get("/.well-known/oauth-authorization-server")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn mcp_metadata_uses_rfc_well_known_paths_for_a_subpath_issuer() {
        for path in [
            "/.well-known/oauth-protected-resource/marginalis/mcp",
            "/.well-known/oauth-authorization-server/marginalis",
        ] {
            let response = subpath_mcp_app()
                .oneshot(Request::get(path).body(Body::empty()).expect("request"))
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }

        let non_standard = subpath_mcp_app()
            .oneshot(
                Request::get("/marginalis/.well-known/oauth-authorization-server")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(non_standard.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn mcp_authorization_starts_login_when_no_web_session_exists() {
        let response = mcp_app()
            .oneshot(
                Request::get(
                    "/oauth/authorize?response_type=code&client_id=client&redirect_uri=http%3A%2F%2F127.0.0.1%3A48123%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                )
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let login_location = response
            .headers()
            .get(header::LOCATION)
            .expect("login location")
            .to_str()
            .expect("valid location")
            .to_owned();
        assert!(login_location.starts_with("/auth/oidc/login?next="));

        let login = mcp_app()
            .oneshot(
                Request::get(login_location)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(login.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            login.headers().get(header::LOCATION).expect("location"),
            "https://id.example.test/authorize"
        );
        assert!(
            login
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .any(|value| value
                    .to_str()
                    .is_ok_and(|value| value.contains(RETURN_TO_COOKIE)))
        );
    }

    #[tokio::test]
    async fn cross_origin_oauth_posts_start_login_without_bypassing_consent_csrf() {
        let form_post = mcp_app()
            .oneshot(
                Request::post("/oauth/authorize")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::ORIGIN, "https://chatgpt.com")
                    .header("sec-fetch-site", "cross-site")
                    .body(Body::from(
                        "response_type=code&client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(form_post.status(), StatusCode::SEE_OTHER);
        assert!(
            form_post
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|location| location.starts_with("/auth/oidc/login?next="))
        );

        let query_post = mcp_app()
            .oneshot(
                Request::post(
                    "/oauth/authorize?response_type=code&client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                )
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(header::ORIGIN, "https://chatgpt.com")
                .header("sec-fetch-site", "cross-site")
                .body(Body::from("csrf_token=client-owned-value"))
                .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(query_post.status(), StatusCode::SEE_OTHER);
        assert!(
            query_post
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|location| location.starts_with("/auth/oidc/login?next="))
        );

        let conflicting_post = mcp_app()
            .oneshot(
                Request::post(
                    "/oauth/authorize?response_type=code&client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                )
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("client_id=different-client"))
                .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(conflicting_post.status(), StatusCode::SEE_OTHER);
        assert!(
            conflicting_post
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|location| location.contains("error=invalid_request"))
        );

        let forged_approval = mcp_app()
            .oneshot(
                Request::post("/oauth/authorize/consent")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::ORIGIN, "https://chatgpt.com")
                    .header("sec-fetch-site", "cross-site")
                    .body(Body::from(
                        "client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&scope=notes%3Aread&code_challenge=verifier&state=opaque&csrf_token=forged&decision=approve",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(forged_approval.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn authorization_errors_redirect_only_after_client_redirect_validation() {
        let invalid_target = mcp_app()
            .oneshot(
                Request::get(
                    "/oauth/authorize?response_type=code&client_id=client&redirect_uri=https%3A%2F%2Fchatgpt.com%2Fconnector%2Foauth%2Fcallback&resource=https%3A%2F%2Fother.example%2Fmcp&code_challenge=verifier&code_challenge_method=S256&state=opaque",
                )
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(invalid_target.status(), StatusCode::SEE_OTHER);
        let location = invalid_target
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("redirect location");
        assert!(location.starts_with("https://chatgpt.com/connector/oauth/callback?"));
        assert!(location.contains("error=invalid_target"));
        assert!(location.contains("state=opaque"));

        let missing_client = mcp_app()
            .oneshot(
                Request::get(
                    "/oauth/authorize?response_type=code&redirect_uri=https%3A%2F%2Fevil.example%2Fcallback",
                )
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(missing_client.status(), StatusCode::BAD_REQUEST);
        assert!(!missing_client.headers().contains_key(header::LOCATION));
    }

    #[tokio::test]
    async fn mcp_requires_a_bearer_token_and_serves_the_tool_catalog() {
        let request = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ))
            .expect("request");
        let denied = mcp_app().oneshot(request).await.expect("response");
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert!(denied.headers().contains_key(header::WWW_AUTHENTICATE));

        let request = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::AUTHORIZATION, "Bearer valid-token")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ))
            .expect("request");
        let allowed = mcp_app().oneshot(request).await.expect("response");
        assert_eq!(allowed.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn mcp_bearer_scheme_is_case_insensitive_and_scope_failures_are_forbidden() {
        let lowercase_bearer = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::AUTHORIZATION, "bearer valid-token")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ))
            .expect("request");
        let allowed = mcp_app().oneshot(lowercase_bearer).await.expect("response");
        assert_eq!(allowed.status(), StatusCode::OK);

        let insufficient_scope = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::AUTHORIZATION, "Bearer read-token")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_note","arguments":{"title":"Title","body":"Body","tags":[]}}}"#,
            ))
            .expect("request");
        let denied = mcp_app()
            .oneshot(insufficient_scope)
            .await
            .expect("response");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert!(
            denied
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    value.contains("error=\"insufficient_scope\"")
                        && value.contains("scope=\"notes:write\"")
                })
        );
    }

    #[tokio::test]
    async fn mcp_accepts_configured_browser_origins_and_rejects_others() {
        let request = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::ORIGIN, "https://chatgpt.com")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ))
            .expect("request");
        let allowed = mcp_app().oneshot(request).await.expect("response");
        assert_eq!(allowed.status(), StatusCode::UNAUTHORIZED);

        let request = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::ORIGIN, "https://example.test")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ))
            .expect("request");
        let same_origin = mcp_app().oneshot(request).await.expect("response");
        assert_eq!(same_origin.status(), StatusCode::UNAUTHORIZED);

        let request = Request::post("/mcp")
            .header("content-type", "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header(header::ORIGIN, "https://untrusted.example")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ))
            .expect("request");
        let rejected = mcp_app().oneshot(request).await.expect("response");
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn mcp_registration_limiter_bounds_a_window() {
        let limiter = McpRegistrationRateLimiter::new(1, Duration::from_secs(60));
        let now = Instant::now();
        assert!(limiter.allow(now));
        assert!(!limiter.allow(now));
        assert!(limiter.allow(now + Duration::from_secs(61)));
    }

    #[test]
    fn browser_mutations_require_the_application_origin() {
        let state = ApiState::new(
            Arc::new(Notes),
            Arc::new(Sessions),
            Arc::new(Oidc),
            "/".into(),
            "https://example.test".into(),
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            "https://example.test".parse().expect("origin"),
        );
        headers.insert("sec-fetch-site", "same-origin".parse().expect("metadata"));
        assert!(validate_mutation_origin(&headers, &state).is_ok());

        headers.insert(
            header::ORIGIN,
            "https://chatgpt.com".parse().expect("origin"),
        );
        assert!(validate_mutation_origin(&headers, &state).is_err());

        headers.insert(
            header::ORIGIN,
            "https://example.test".parse().expect("origin"),
        );
        headers.insert("sec-fetch-site", "cross-site".parse().expect("metadata"));
        assert!(validate_mutation_origin(&headers, &state).is_err());
    }

    #[tokio::test]
    async fn oauth_consent_uses_session_bound_csrf_when_client_context_has_an_opaque_origin() {
        let state = ApiState::new(
            Arc::new(Notes),
            Arc::new(ActiveSessions),
            Arc::new(Oidc),
            "/".into(),
            "https://example.test".into(),
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "marginalis_session=active-session; marginalis_csrf=session-csrf"
                .parse()
                .expect("cookies"),
        );
        headers.insert(header::ORIGIN, "null".parse().expect("opaque origin"));
        headers.insert("sec-fetch-site", "cross-site".parse().expect("metadata"));

        assert!(
            authenticated_form_actor(&headers, &state, "session-csrf")
                .await
                .is_ok()
        );
        assert!(
            authenticated_form_actor(&headers, &state, "forged")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn mcp_dynamic_registration_creates_a_public_client() {
        let response = mcp_app()
            .oneshot(
                Request::post("/oauth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"client_name":"Claude Code","redirect_uris":["http://localhost:48123/callback"]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn mcp_registration_reports_invalid_redirect_uri() {
        let response = mcp_app()
            .oneshot(
                Request::post("/oauth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"client_name":"Invalid","redirect_uris":["http://remote.example/callback"]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let error: serde_json::Value = serde_json::from_slice(&body).expect("OAuth error");
        assert_eq!(error["error"], "invalid_redirect_uri");
    }

    #[tokio::test]
    async fn mcp_token_response_is_not_cacheable() {
        let response = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=authorization_code&code=code&client_id=client&redirect_uri=http%3A%2F%2F127.0.0.1%2Fcallback&resource=https%3A%2F%2Fexample.test%2Fmcp&code_verifier=verifier",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&"no-store".parse().expect("header"))
        );
        assert_eq!(
            response.headers().get(header::PRAGMA),
            Some(&"no-cache".parse().expect("header"))
        );
    }

    #[tokio::test]
    async fn mcp_token_errors_use_oauth_error_shape() {
        let response = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=password&client_id=client&resource=https%3A%2F%2Fexample.test%2Fmcp",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&"no-store".parse().expect("header"))
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let error: serde_json::Value = serde_json::from_slice(&body).expect("OAuth error");
        assert_eq!(error["error"], "unsupported_grant_type");
    }

    #[tokio::test]
    async fn mcp_token_accepts_an_omitted_redirect_and_rejects_duplicate_parameters() {
        let omitted_redirect = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=authorization_code&code=code&client_id=client&resource=https%3A%2F%2Fexample.test%2Fmcp&code_verifier=verifier",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(omitted_redirect.status(), StatusCode::OK);

        let duplicate = mcp_app()
            .oneshot(
                Request::post("/oauth/token")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=authorization_code&grant_type=refresh_token&client_id=client&resource=https%3A%2F%2Fexample.test%2Fmcp",
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(duplicate.into_body(), usize::MAX)
            .await
            .expect("body");
        let error: serde_json::Value = serde_json::from_slice(&body).expect("OAuth error");
        assert_eq!(error["error"], "invalid_request");
    }

    #[tokio::test]
    async fn public_mcp_endpoints_reject_oversized_request_bodies() {
        let registration = mcp_app()
            .oneshot(
                Request::post("/oauth/register")
                    .header("content-type", "application/json")
                    .body(Body::from(vec![b' '; 16 * 1024 + 1]))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(registration.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let mcp = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(vec![b' '; 1024 * 1024 + 1]))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(mcp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
