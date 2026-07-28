use super::*;
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request},
};
use marginalis_application::{
    AuthenticationUseCaseError, McpAuthorizationClient, McpOAuthUseCaseError, McpOAuthUseCases,
    McpTokenPair, McpValidatedAuthorizationRequest, NoteAccessControl, NoteAclChange, NoteAclState,
    NoteCommands, NotePresentation, NoteProfile, NoteProfileExample, NoteProfileLimits,
    NoteProfileNormalization, NoteProfileSyntax, NoteQueries, NoteRenderContext, NoteUseCaseError,
    NoteValidationCode, NoteValidationDiagnostic, NoteValidationTarget, NoteView,
    OidcAuthenticationUseCases, RelatedNotes, Utf8ByteSpan, WebSessionUseCases,
};
use marginalis_domain::{
    Actor, AuthenticatedSession, Identity, McpAuthenticatedActor, McpOAuthClient, Note, NoteAccess,
    NoteDraft, NoteId, NoteListEntry, NoteSummary, Revision, UnixMillis, WebSession,
};
use std::time::{Duration, Instant};
use tower::ServiceExt;

macro_rules! implement_note_boundaries {
    ($type:ty) => {
        #[async_trait]
        impl NoteQueries for $type {
            async fn list_visible_notes(
                &self,
                actor: Actor,
            ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
                <$type>::list_visible_notes(self, actor).await
            }

            async fn read_note(
                &self,
                actor: Actor,
                note_id: NoteId,
            ) -> Result<Note, NoteUseCaseError> {
                <$type>::read_note(self, actor, note_id).await
            }
        }

        #[async_trait]
        impl NoteCommands for $type {
            async fn create_note(
                &self,
                actor: Actor,
                draft: NoteDraft,
            ) -> Result<Note, NoteUseCaseError> {
                <$type>::create_note(self, actor, draft).await
            }

            async fn update_note(
                &self,
                actor: Actor,
                note_id: NoteId,
                draft: NoteDraft,
                expected_revision: Revision,
            ) -> Result<Note, NoteUseCaseError> {
                <$type>::update_note(self, actor, note_id, draft, expected_revision).await
            }

            async fn soft_delete_note(
                &self,
                actor: Actor,
                note_id: NoteId,
                expected_revision: Revision,
            ) -> Result<Note, NoteUseCaseError> {
                <$type>::soft_delete_note(self, actor, note_id, expected_revision).await
            }

            async fn restore_note(
                &self,
                actor: Actor,
                note_id: NoteId,
                expected_revision: Revision,
            ) -> Result<Note, NoteUseCaseError> {
                <$type>::restore_note(self, actor, note_id, expected_revision).await
            }
        }

        #[async_trait]
        impl NotePresentation for $type {
            async fn preview_note(
                &self,
                actor: Actor,
                draft: NoteDraft,
                context: NoteRenderContext,
            ) -> Result<String, NoteUseCaseError> {
                <$type>::preview_note(self, actor, draft, context).await
            }

            fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError> {
                <$type>::export_note_source(self, note)
            }

            async fn read_note_view(
                &self,
                actor: Actor,
                note_id: NoteId,
                context: NoteRenderContext,
            ) -> Result<NoteView, NoteUseCaseError> {
                <$type>::read_note_view(self, actor, note_id, context).await
            }

            fn note_profile(&self) -> NoteProfile {
                <$type>::note_profile(self)
            }
        }

        #[async_trait]
        impl NoteAccessControl for $type {
            async fn read_note_acl(
                &self,
                actor: Actor,
                note_id: NoteId,
            ) -> Result<NoteAclState, NoteUseCaseError> {
                <$type>::read_note_acl(self, actor, note_id).await
            }

            async fn replace_note_acl(
                &self,
                actor: Actor,
                note_id: NoteId,
                entries: Vec<NoteAclChange>,
                expected_revision: Revision,
            ) -> Result<Note, NoteUseCaseError> {
                <$type>::replace_note_acl(self, actor, note_id, entries, expected_revision).await
            }
        }
    };
}

use super::{
    auth::{
        RETURN_TO_COOKIE, authenticated_form_actor, external_path, valid_return_to,
        validate_mutation_origin,
    },
    state::McpRegistrationRateLimiter,
};

struct Notes;

impl Notes {
    async fn list_visible_notes(
        &self,
        _actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        Ok(Vec::new())
    }

    async fn read_note(&self, _actor: Actor, _note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    async fn create_note(&self, _actor: Actor, draft: NoteDraft) -> Result<Note, NoteUseCaseError> {
        if !draft.source.starts_with("= ") {
            return Err(NoteUseCaseError::Validation(vec![
                NoteValidationDiagnostic {
                    code: NoteValidationCode::InvalidTitle,
                    target: NoteValidationTarget::Source,
                    span: None,
                    message: "title is invalid",
                },
                NoteValidationDiagnostic {
                    code: NoteValidationCode::UnsupportedSourceLanguage,
                    target: NoteValidationTarget::Source,
                    span: Some(Utf8ByteSpan { start: 8, end: 13 }),
                    message: "source language is not allowed",
                },
            ]));
        }
        Err(NoteUseCaseError::Unavailable)
    }

    async fn update_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _draft: NoteDraft,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    async fn preview_note(
        &self,
        _actor: Actor,
        draft: NoteDraft,
        _context: NoteRenderContext,
    ) -> Result<String, NoteUseCaseError> {
        if !draft.source.starts_with("= ") {
            Err(NoteUseCaseError::Validation(vec![
                NoteValidationDiagnostic {
                    code: NoteValidationCode::InvalidTitle,
                    target: NoteValidationTarget::Source,
                    span: None,
                    message: "title is invalid",
                },
            ]))
        } else {
            Ok("<article><p>プレビュー</p></article>".into())
        }
    }

    async fn soft_delete_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    async fn restore_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    fn export_note_source(&self, _note: &Note) -> Result<String, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    async fn read_note_view(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _context: NoteRenderContext,
    ) -> Result<NoteView, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    async fn read_note_acl(
        &self,
        _actor: Actor,
        _note_id: NoteId,
    ) -> Result<NoteAclState, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    async fn replace_note_acl(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _entries: Vec<NoteAclChange>,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    fn note_profile(&self) -> NoteProfile {
        NoteProfile {
            profile_version: 2,
            adocweave_package_version: "0.11.0",
            limits: NoteProfileLimits {
                max_title_characters: 200,
                max_source_bytes: 524_288,
                max_tags: 50,
                max_tag_characters: 64,
            },
            normalization: NoteProfileNormalization {
                title: vec!["trim", "unicode_nfc"],
                tags: vec!["trim", "unicode_nfc"],
            },
            syntax: NoteProfileSyntax {
                common_blocks: vec!["paragraph"],
                common_inlines: Vec::new(),
                source_language_optional: true,
                allowed_math_languages: vec!["latexmath"],
                title_forbidden: vec!["empty"],
                tag_forbidden: vec!["empty"],
            },
            allowed_source_languages: vec!["rust"],
            forbidden_rules: Vec::new(),
            examples: vec![NoteProfileExample {
                kind: "paragraph",
                description: "Paragraph",
                body: "Body.",
            }],
        }
    }
}

struct UiNotes {
    notes: Vec<Note>,
    render_fails: bool,
}

impl UiNotes {
    async fn list_visible_notes(
        &self,
        _actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        Ok(self
            .notes
            .iter()
            .map(|note| NoteListEntry {
                summary: NoteSummary::from(note),
                access: NoteAccess::Edit,
            })
            .collect())
    }

    async fn read_note(&self, _actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.notes
            .iter()
            .find(|note| note.note_id() == note_id)
            .cloned()
            .ok_or(NoteUseCaseError::NotFound)
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
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    async fn preview_note(
        &self,
        _actor: Actor,
        _draft: NoteDraft,
        _context: NoteRenderContext,
    ) -> Result<String, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    async fn soft_delete_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    async fn restore_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    fn export_note_source(&self, _note: &Note) -> Result<String, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    async fn read_note_view(
        &self,
        actor: Actor,
        note_id: NoteId,
        _context: NoteRenderContext,
    ) -> Result<NoteView, NoteUseCaseError> {
        let note = self.read_note(actor, note_id).await?;
        if self.render_fails {
            Err(NoteUseCaseError::RenderFailed)
        } else {
            let related = self
                .notes
                .iter()
                .filter(|candidate| candidate.note_id() != note_id)
                .map(NoteSummary::from)
                .collect::<Vec<_>>();
            Ok(NoteView {
                note,
                access: NoteAccess::Edit,
                html: "<article><p>描画済み本文</p></article>".into(),
                related: RelatedNotes {
                    outgoing: related.clone(),
                    incoming: related,
                },
            })
        }
    }

    async fn read_note_acl(
        &self,
        _actor: Actor,
        _note_id: NoteId,
    ) -> Result<NoteAclState, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    async fn replace_note_acl(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _entries: Vec<NoteAclChange>,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    fn note_profile(&self) -> NoteProfile {
        Notes.note_profile()
    }
}

implement_note_boundaries!(Notes);
implement_note_boundaries!(UiNotes);

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

    async fn issue_session(&self, _actor: Actor) -> Result<WebSession, AuthenticationUseCaseError> {
        Err(AuthenticationUseCaseError::Unavailable)
    }

    async fn revoke_session(&self, _session_id: String) -> Result<(), AuthenticationUseCaseError> {
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
                actor: Actor::try_new("https://id.example.test".into(), "alice".into())
                    .expect("valid actor"),
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

    async fn issue_session(&self, _actor: Actor) -> Result<WebSession, AuthenticationUseCaseError> {
        Err(AuthenticationUseCaseError::Unavailable)
    }

    async fn revoke_session(&self, _session_id: String) -> Result<(), AuthenticationUseCaseError> {
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
    async fn register_client(&self, client: McpOAuthClient) -> Result<(), McpOAuthUseCaseError> {
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
        refresh_token: String,
        _client_id: String,
        _resource_uri: String,
        scopes: Option<Vec<String>>,
    ) -> Result<McpTokenPair, McpOAuthUseCaseError> {
        if refresh_token == "refresh-ok" {
            Ok(McpTokenPair {
                access_token: "downscoped-access".into(),
                refresh_token: "rotated-refresh".into(),
                access_expires_in_seconds: 300,
                scope: scopes
                    .unwrap_or_else(|| vec!["notes:read".into()])
                    .join(" "),
            })
        } else {
            Err(McpOAuthUseCaseError::InvalidGrant)
        }
    }
    async fn authenticate(
        &self,
        token: String,
        _resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError> {
        Ok(
            matches!(token.as_str(), "valid-token" | "read-token" | "write-token").then(|| {
                McpAuthenticatedActor {
                    actor: Actor::try_new("https://kanidm.example.test".into(), "alice".into())
                        .expect("valid actor"),
                    scopes: match token.as_str() {
                        "read-token" => vec!["notes:read".into()],
                        "write-token" => vec!["notes:write".into()],
                        _ => vec![
                            "notes:read".into(),
                            "notes:write".into(),
                            "notes:delete".into(),
                        ],
                    },
                }
            }),
        )
    }
    async fn revoke(&self, _actor: Actor, _client_id: String) -> Result<(), McpOAuthUseCaseError> {
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
    let base_url = url::Url::parse("https://example.test").expect("base URL");
    router(
        ApiState::new(
            Arc::new(Notes),
            Arc::new(Sessions),
            Arc::new(Oidc),
            "/".into(),
            "https://example.test".into(),
        )
        .with_mcp(McpEndpoint::new(
            Arc::new(Mcp),
            &base_url,
            vec!["https://chatgpt.com".into()],
        )),
    )
}

fn authenticated_app() -> Router {
    router(ApiState::new(
        Arc::new(Notes),
        Arc::new(ActiveSessions),
        Arc::new(Oidc),
        "/".into(),
        "https://example.test".into(),
    ))
}

fn ui_note(title: &str) -> Note {
    Note::restore(
        NoteId::new(
            "0197c9bc-0000-7000-8000-000000000001"
                .parse()
                .expect("note ID"),
        ),
        Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
        title.into(),
        "本文".into(),
        vec!["試験".into()],
        UnixMillis::new(1),
        UnixMillis::new(2),
        Revision::INITIAL,
        None,
    )
    .expect("consistent note")
}

fn ui_app(notes: Vec<Note>, render_fails: bool, cookie_path: &str) -> Router {
    router(ApiState::new(
        Arc::new(UiNotes {
            notes,
            render_fails,
        }),
        Arc::new(ActiveSessions),
        Arc::new(Oidc),
        cookie_path.into(),
        "https://example.test".into(),
    ))
}

fn authenticated_request(uri: &str) -> Request<Body> {
    Request::get(uri)
        .header(header::COOKIE, "marginalis_session=active-session")
        .body(Body::empty())
        .expect("request")
}

fn subpath_mcp_app() -> Router {
    let base_url = url::Url::parse("https://example.test/marginalis").expect("base URL");
    router(
        ApiState::new(
            Arc::new(Notes),
            Arc::new(Sessions),
            Arc::new(Oidc),
            "/marginalis".into(),
            "https://example.test".into(),
        )
        .with_mcp(McpEndpoint::new(Arc::new(Mcp), &base_url, vec![])),
    )
}

mod ui_contracts {
    use super::*;

    include!("tests/ui_contracts.rs");
}

mod mcp_transport {
    use super::*;

    include!("tests/mcp_transport.rs");
}

mod rest_notes {
    use super::*;

    include!("tests/rest_notes.rs");
}

mod oauth {
    use super::*;

    include!("tests/oauth.rs");
}
