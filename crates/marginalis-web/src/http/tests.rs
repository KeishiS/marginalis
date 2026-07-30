use super::*;
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Request},
};
use marginalis_application::{
    AuthenticationUseCaseError, McpAccessTokenAuthenticationError, McpAccessTokenAuthenticator,
    NoteAccessControl, NoteAclChange, NoteAclState, NoteAdvisoryDiagnostic, NoteAdvisorySeverity,
    NoteCommands, NotePresentation, NotePreview, NoteProfile, NoteProfileExample,
    NoteProfileLimits, NoteProfileNormalization, NoteProfileSyntax, NoteQueries, NoteRenderContext,
    NoteUseCaseError, NoteValidationCode, NoteValidationDiagnostic, NoteValidationTarget, NoteView,
    NoteWritePolicy, OidcAuthenticationUseCases, RelatedNotes, Utf8ByteSpan, WebSessionUseCases,
};
use marginalis_contract::McpNoteMutationOutput;
use marginalis_domain::{
    Actor, AuthenticatedSession, Identity, McpAuthenticatedActor, Note, NoteAccess, NoteDraft,
    NoteId, NoteListEntry, NoteSummary, Revision, UnixMillis, WebSession,
};
use std::{
    io,
    sync::{Mutex, OnceLock},
};
use tower::ServiceExt;
use tracing_subscriber::fmt::MakeWriter;

#[test]
fn http_observability_classifies_response_outcomes() {
    assert_eq!(http_outcome(StatusCode::OK), "success");
    assert_eq!(http_outcome(StatusCode::FOUND), "success");
    assert_eq!(http_outcome(StatusCode::NOT_FOUND), "rejected");
    assert_eq!(http_outcome(StatusCode::SERVICE_UNAVAILABLE), "failure");
}

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl CapturedLogs {
    fn clear(&self) {
        self.0.lock().expect("captured logs").clear();
    }

    fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("captured logs").clone()).expect("UTF-8 logs")
    }
}

impl io::Write for CapturedLogs {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("captured log lock was poisoned"))?
            .extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'writer> MakeWriter<'writer> for CapturedLogs {
    type Writer = Self;

    fn make_writer(&'writer self) -> Self::Writer {
        self.clone()
    }
}

fn global_captured_logs() -> CapturedLogs {
    static LOGS: OnceLock<CapturedLogs> = OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_target(false)
            .compact()
            .with_writer(logs.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("install captured log subscriber");
        logs
    })
    .clone()
}

fn assert_log_line(logs: &str, expected_fields: &[&str]) {
    assert!(
        logs.lines()
            .any(|line| expected_fields.iter().all(|field| line.contains(field))),
        "次のfieldを同じログ行で確認できませんでした: {expected_fields:?}\n{logs}"
    );
}

#[test]
fn observability_logs_safe_http_and_mcp_results() {
    let logs = global_captured_logs();
    logs.clear();
    let note_id = "0197c9bc-0000-7000-8000-000000000001";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let response = app()
            .oneshot(
                Request::get(format!(
                    "/api/v3/notes/{note_id}?search=must-not-be-logged"
                ))
                .header(header::COOKIE, "marginalis_session=secret-cookie")
                .body(Body::empty())
                .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer secret-bearer")
                    .body(Body::from("not-json-secret"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Basic secret-basic")
                    .body(Body::from("malformed-auth-body"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer read-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":"list-id","method":"tools/call","params":{"name":"list_notes"}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer write-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":"unavailable-id","method":"tools/call","params":{"name":"create_note","arguments":{"source":"= Private title\n\nPrivate body"}}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = mcp_app()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::AUTHORIZATION, "Bearer write-token")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":"private-id","method":"tools/call","params":{"name":"create_note","arguments":{"source":"private source"}}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    });

    let logs = logs.text();
    assert_log_line(
        &logs,
        &[
            "event=\"http.request.completed\"",
            "request_id=",
            "method=GET",
            "path=\"/api/v3/notes/{note_id}\"",
            "problem_code=\"authentication_required\"",
            "status=401",
            "outcome=\"rejected\"",
            "latency_ms=",
        ],
    );
    assert_log_line(
        &logs,
        &[
            "event=\"mcp.request.completed\"",
            "method=\"unknown\"",
            "outcome=\"rejected\"",
            "reason=\"parse-error\"",
        ],
    );
    assert_log_line(
        &logs,
        &[
            "event=\"mcp.authentication.failed\"",
            "reason=\"token-format\"",
        ],
    );
    assert_log_line(
        &logs,
        &[
            "event=\"mcp.tool.completed\"",
            "tool=\"list_notes\"",
            "outcome=\"success\"",
        ],
    );
    assert_log_line(
        &logs,
        &[
            "event=\"mcp.tool.completed\"",
            "tool=\"create_note\"",
            "outcome=\"failure\"",
            "reason=\"unavailable\"",
        ],
    );
    assert_log_line(
        &logs,
        &[
            "event=\"mcp.tool.completed\"",
            "tool=\"create_note\"",
            "outcome=\"rejected\"",
            "reason=\"validation\"",
        ],
    );
    for secret in [
        note_id,
        "must-not-be-logged",
        "secret-cookie",
        "secret-bearer",
        "not-json-secret",
        "private-id",
        "private source",
        "secret-basic",
        "malformed-auth-body",
        "list-id",
        "unavailable-id",
        "Private title",
        "Private body",
    ] {
        assert!(
            !logs.contains(secret),
            "logs contain secret fixture: {secret}"
        );
    }
}

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
                policy: NoteWritePolicy,
            ) -> Result<Note, NoteUseCaseError> {
                <$type>::create_note(self, actor, draft, policy).await
            }

            async fn update_note(
                &self,
                actor: Actor,
                note_id: NoteId,
                draft: NoteDraft,
                expected_revision: Revision,
                policy: NoteWritePolicy,
            ) -> Result<Note, NoteUseCaseError> {
                <$type>::update_note(self, actor, note_id, draft, expected_revision, policy).await
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
            ) -> Result<NotePreview, NoteUseCaseError> {
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

use super::auth::{external_path, valid_return_to, validate_mutation_origin};

struct Notes;

fn test_advisories(source: &str) -> Vec<NoteAdvisoryDiagnostic> {
    source.find("xref").map_or_else(Vec::new, |start| {
        vec![
            NoteAdvisoryDiagnostic {
                code: "macro-boundary".into(),
                severity: NoteAdvisorySeverity::Warning,
                target: NoteValidationTarget::Source,
                span: Some(Utf8ByteSpan {
                    start: start as u32,
                    end: start as u32 + 4,
                }),
                message: "a space is required before the inline macro".into(),
            },
            NoteAdvisoryDiagnostic {
                code: "document-information".into(),
                severity: NoteAdvisorySeverity::Information,
                target: NoteValidationTarget::Source,
                span: None,
                message: "document information".into(),
            },
            NoteAdvisoryDiagnostic {
                code: "document-hint".into(),
                severity: NoteAdvisorySeverity::Hint,
                target: NoteValidationTarget::Source,
                span: None,
                message: "document hint".into(),
            },
        ]
    })
}

fn reject_test_warnings(source: &str, policy: NoteWritePolicy) -> Result<(), NoteUseCaseError> {
    let diagnostics = test_advisories(source);
    if policy == NoteWritePolicy::RejectWarnings
        && diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == NoteAdvisorySeverity::Warning)
    {
        Err(NoteUseCaseError::AdvisoriesRejected(diagnostics))
    } else {
        Ok(())
    }
}

impl Notes {
    async fn list_visible_notes(
        &self,
        _actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        let note = mcp_note();
        Ok(vec![NoteListEntry {
            summary: NoteSummary::from(&note),
            access: NoteAccess::Manage,
        }])
    }

    async fn read_note(&self, _actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        let note = mcp_note();
        if note.note_id() == note_id {
            Ok(note)
        } else {
            Err(NoteUseCaseError::NotFound)
        }
    }

    async fn create_note(
        &self,
        _actor: Actor,
        draft: NoteDraft,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        if !draft.source.starts_with("= ") {
            return Err(NoteUseCaseError::Validation(vec![
                NoteValidationDiagnostic {
                    code: NoteValidationCode::InvalidTitle.as_str().into(),
                    target: NoteValidationTarget::Source,
                    span: None,
                    message: "title is invalid".into(),
                },
                NoteValidationDiagnostic {
                    code: NoteValidationCode::UnsupportedSourceLanguage
                        .as_str()
                        .into(),
                    target: NoteValidationTarget::Source,
                    span: Some(Utf8ByteSpan { start: 8, end: 13 }),
                    message: "source language is not allowed".into(),
                },
            ]));
        }
        reject_test_warnings(&draft.source, policy)?;
        Err(NoteUseCaseError::Unavailable)
    }

    async fn update_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        draft: NoteDraft,
        _expected_revision: Revision,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        reject_test_warnings(&draft.source, policy)?;
        Err(NoteUseCaseError::Unavailable)
    }

    async fn preview_note(
        &self,
        _actor: Actor,
        draft: NoteDraft,
        _context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        if !draft.source.starts_with("= ") {
            Err(NoteUseCaseError::Validation(vec![
                NoteValidationDiagnostic {
                    code: NoteValidationCode::InvalidTitle.as_str().into(),
                    target: NoteValidationTarget::Source,
                    span: None,
                    message: "title is invalid".into(),
                },
            ]))
        } else {
            let diagnostics = test_advisories(&draft.source);
            Ok(NotePreview {
                html: "<article><p>プレビュー</p></article>".into(),
                diagnostics,
            })
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
        const BIBLIOGRAPHY_GUIDANCE: &str = "Use bibliographic metadata supplied by the user or an identified source. Never invent or infer authors, titles, publication years, DOIs, or other bibliographic metadata.";
        const BIBLIOGRAPHY_EXAMPLE: &str = "= 先行研究の整理\n:tags: 文献, 研究\n\nSmithらは、対象の手法が有効だと報告しています <<smith2024>>。\n\n[bibliography]\n== 参考文献\n\n* [[[smith2024]]] Smith, A. et al. _Example Paper_. Example Journal, 2024. https://doi.org/10.1234/replace-with-doi[DOI]";
        NoteProfile {
            profile_version: 6,
            adocweave_package_version: "0.20.0",
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
            authoring_guidance: vec![BIBLIOGRAPHY_GUIDANCE],
            allowed_source_languages: vec!["rust"],
            forbidden_rules: Vec::new(),
            examples: vec![NoteProfileExample {
                kind: "bibliography",
                description: "Complete document with a bibliography entry and an in-text reference",
                body: BIBLIOGRAPHY_EXAMPLE,
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
        _policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    async fn update_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _draft: NoteDraft,
        _expected_revision: Revision,
        _policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    async fn preview_note(
        &self,
        _actor: Actor,
        _draft: NoteDraft,
        _context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
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

struct TestMcpAuthenticator;

#[async_trait]
impl McpAccessTokenAuthenticator for TestMcpAuthenticator {
    async fn authenticate_access_token(
        &self,
        token: String,
        resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpAccessTokenAuthenticationError> {
        Ok((matches!(
            token.as_str(),
            "external-token" | "valid-token" | "read-token" | "write-token"
        ) && resource_uri.ends_with("/mcp"))
        .then(|| McpAuthenticatedActor {
            actor: Actor::try_new("https://kanidm.example.test".into(), "alice".into())
                .expect("valid actor"),
            scopes: match token.as_str() {
                "read-token" | "external-token" => vec!["notes:read".into()],
                "write-token" => vec!["notes:write".into()],
                _ => vec![
                    "notes:read".into(),
                    "notes:write".into(),
                    "notes:delete".into(),
                ],
            },
        }))
    }
}

struct UnavailableMcpAuthenticator;

#[async_trait]
impl McpAccessTokenAuthenticator for UnavailableMcpAuthenticator {
    async fn authenticate_access_token(
        &self,
        _token: String,
        _resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpAccessTokenAuthenticationError> {
        Err(McpAccessTokenAuthenticationError::Unavailable)
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
    mcp_app_with_authenticator(Arc::new(TestMcpAuthenticator))
}

fn mcp_app_with_authenticator(authenticator: Arc<dyn McpAccessTokenAuthenticator>) -> Router {
    let base_url = url::Url::parse("https://example.test").expect("base URL");
    router(
        ApiState::new(
            Arc::new(Notes),
            Arc::new(Sessions),
            Arc::new(Oidc),
            "/".into(),
            "https://example.test".into(),
        )
        .with_mcp(
            McpEndpoint::new(
                &base_url,
                vec!["https://chatgpt.com".into()],
                "https://issuer.example.test/".into(),
                authenticator,
            )
            .expect("MCP endpoint"),
        ),
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

fn mcp_note() -> Note {
    Note::restore(
        NoteId::new(
            "0197c9bc-0000-7000-8000-000000000002"
                .parse()
                .expect("note ID"),
        ),
        Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
        "同期ノート".into(),
        "= 同期ノート\n:tags: 同期, 試験\n\n本文".into(),
        vec!["同期".into(), "試験".into()],
        UnixMillis::new(1_000),
        UnixMillis::new(2_000),
        Revision::new(3).expect("revision"),
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
        .with_mcp(
            McpEndpoint::new(
                &base_url,
                vec![],
                "https://issuer.example.test/".into(),
                Arc::new(TestMcpAuthenticator),
            )
            .expect("MCP endpoint"),
        ),
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
