use super::*;
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Request},
};
use marginalis_application::{
    AuthenticationUseCaseError, McpAuthorizationClient, McpClientRegistrationMethod,
    McpOAuthUseCaseError, McpOAuthUseCases, McpTokenPair, McpValidatedAuthorizationRequest,
    NoteAccessControl, NoteAclChange, NoteAclState, NoteAdvisoryDiagnostic, NoteAdvisorySeverity,
    NoteCommands, NoteGraph, NoteGraphNote, NoteGraphQuery, NotePresentation, NotePreview,
    NoteProfile, NoteProfileExample, NoteProfileLimits, NoteProfileNormalization,
    NoteProfileSyntax, NoteQueries, NoteRenderContext, NoteUseCaseError, NoteUseCases,
    NoteValidationCode, NoteValidationDiagnostic, NoteView, NoteWritePolicy,
    OidcAuthenticationUseCases, RelatedNotes, WebSessionUseCases,
};
use marginalis_contract::McpNoteMutationOutput;
use marginalis_domain::{
    Actor, AuthenticatedSession, Identity, McpAuthenticatedActor, McpOAuthClient, Note, NoteAccess,
    NoteDraft, NoteId, NoteListEntry, NoteSummary, NoteValidationTarget, Revision, UnixMillis,
    Utf8ByteSpan, WebSession,
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
            async fn preview_new_note(
                &self,
                actor: Actor,
                draft: NoteDraft,
                context: NoteRenderContext,
            ) -> Result<NotePreview, NoteUseCaseError> {
                <$type>::preview_new_note(self, actor, draft, context).await
            }

            async fn preview_note_update(
                &self,
                actor: Actor,
                note_id: NoteId,
                draft: NoteDraft,
                context: NoteRenderContext,
            ) -> Result<NotePreview, NoteUseCaseError> {
                <$type>::preview_note_update(self, actor, note_id, draft, context).await
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

            async fn read_note_graph(
                &self,
                actor: Actor,
                query: NoteGraphQuery,
            ) -> Result<NoteGraph, NoteUseCaseError> {
                <$type>::read_note_graph(self, actor, query).await
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

    async fn preview_new_note(
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

    async fn preview_note_update(
        &self,
        actor: Actor,
        _note_id: NoteId,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        self.preview_new_note(actor, draft, context).await
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

    async fn read_note_graph(
        &self,
        _actor: Actor,
        _query: NoteGraphQuery,
    ) -> Result<NoteGraph, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
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
        const BIBLIOGRAPHY_EXAMPLE: &str = "= 先行研究の整理\n:marginalis-tags: 文献, 研究\n\nSmithらは、対象の手法が有効だと報告しています <<smith2024>>。\n\n[bibliography]\n== 参考文献\n\n* [[[smith2024]]] Smith, A. et al. _Example Paper_. Example Journal, 2024. https://doi.org/10.1234/replace-with-doi[DOI]";
        NoteProfile {
            profile_version: 6,
            adocweave_package_version: "0.23.0",
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
                allowed_document_attributes: vec!["marginalis-tags", "sectnums"],
                allowed_citation_styles: vec!["author-year"],
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

    async fn preview_new_note(
        &self,
        _actor: Actor,
        _draft: NoteDraft,
        _context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    async fn preview_note_update(
        &self,
        actor: Actor,
        _note_id: NoteId,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        self.preview_new_note(actor, draft, context).await
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

    /// 画面試験のために、保持しているノートをそのまま点として返す。
    ///
    /// 実装が無いと、生成した`NotePresentation`が自分自身を呼び戻して止まらなくなる。
    async fn read_note_graph(
        &self,
        _actor: Actor,
        query: NoteGraphQuery,
    ) -> Result<NoteGraph, NoteUseCaseError> {
        let text = query.text.unwrap_or_default();
        Ok(NoteGraph {
            notes: self
                .notes
                .iter()
                .filter(|note| text.is_empty() || note.title().contains(&text))
                .map(|note| NoteGraphNote {
                    note_id: note.note_id(),
                    title: note.title().to_owned(),
                    tags: note.tags().to_vec(),
                    updated_at: note.updated_at(),
                })
                .collect(),
            ..NoteGraph::default()
        })
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

/// MCP access tokenの検証結果だけを差し替える試験用のport。
///
/// 実装は`McpOAuthUseCases::authenticate`と同じ形にし、transportの試験が認可全体の
/// スタブを書き分けずに済むようにする。
#[async_trait]
trait TestMcpAccessTokens: Send + Sync {
    async fn authenticate_access_token(
        &self,
        token: String,
        resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError>;
}

struct TestMcpAuthenticator;

#[async_trait]
impl TestMcpAccessTokens for TestMcpAuthenticator {
    async fn authenticate_access_token(
        &self,
        token: String,
        resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError> {
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
impl TestMcpAccessTokens for UnavailableMcpAuthenticator {
    async fn authenticate_access_token(
        &self,
        _token: String,
        _resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError> {
        Err(McpOAuthUseCaseError::Unavailable)
    }
}

struct TestMcpOAuth {
    authenticator: Arc<dyn TestMcpAccessTokens>,
}

#[async_trait]
impl McpOAuthUseCases for TestMcpOAuth {
    async fn register_client(&self, client: McpOAuthClient) -> Result<(), McpOAuthUseCaseError> {
        if client
            .redirect_uris
            .iter()
            .any(|uri| uri.starts_with("http://remote.example"))
        {
            Err(McpOAuthUseCaseError::InvalidRedirectUri)
        } else if client
            .redirect_uris
            .iter()
            .any(|uri| uri.starts_with("https://at-capacity.example"))
        {
            Err(McpOAuthUseCaseError::Capacity)
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
            registration_method: McpClientRegistrationMethod::Dynamic,
            redirect_uri,
        })
    }

    async fn validate_authorization_request(
        &self,
        request: marginalis_application::McpAuthorizationRequest,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthUseCaseError> {
        if request.client_id == "resolved-only-client" {
            return Err(McpOAuthUseCaseError::Unavailable);
        }
        let resolved = self
            .resolve_authorization_client(request.client_id.clone(), request.redirect_uri.clone())
            .await?;
        self.validate_resolved_authorization_request(request, resolved)
            .await
    }

    async fn validate_resolved_authorization_request(
        &self,
        request: marginalis_application::McpAuthorizationRequest,
        resolved: McpAuthorizationClient,
    ) -> Result<McpValidatedAuthorizationRequest, McpOAuthUseCaseError> {
        if request.resource_uri != "https://example.test/mcp" {
            return Err(McpOAuthUseCaseError::InvalidTarget);
        }
        Ok(McpValidatedAuthorizationRequest {
            client: resolved.client,
            registration_method: resolved.registration_method,
            redirect_uri: resolved.redirect_uri,
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
        if refresh_token != "refresh-ok" {
            return Err(McpOAuthUseCaseError::InvalidGrant);
        }
        Ok(McpTokenPair {
            access_token: "downscoped-access".into(),
            refresh_token: "rotated-refresh".into(),
            access_expires_in_seconds: 300,
            scope: scopes
                .unwrap_or_else(|| vec!["notes:read".into()])
                .join(" "),
        })
    }

    async fn authenticate(
        &self,
        token: String,
        resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError> {
        self.authenticator
            .authenticate_access_token(token, resource_uri)
            .await
    }

    async fn revoke(&self, _actor: Actor, client_id: String) -> Result<(), McpOAuthUseCaseError> {
        if matches!(
            client_id.as_str(),
            "unavailable-client" | "https://client.example.test/metadata.json"
        ) {
            Err(McpOAuthUseCaseError::Unavailable)
        } else {
            Ok(())
        }
    }

    async fn revoke_token(
        &self,
        _token: String,
        _client_id: String,
    ) -> Result<(), McpOAuthUseCaseError> {
        Ok(())
    }
}

/// 試験用のrouterを組み立てる。既定から異なる部分だけを指定する。
///
/// 以前は6つのbuilderが`ApiState::new`の同じ組み立てをそれぞれ書いており、共通部分を変更するには
/// すべてを直す必要があった。
struct TestApp {
    notes: Arc<dyn NoteUseCases>,
    sessions: Arc<dyn WebSessionUseCases>,
    cookie_path: String,
    mcp: Option<(&'static str, Vec<String>, Arc<dyn TestMcpAccessTokens>)>,
}

impl Default for TestApp {
    fn default() -> Self {
        Self {
            notes: Arc::new(Notes),
            sessions: Arc::new(Sessions),
            cookie_path: "/".into(),
            mcp: None,
        }
    }
}

impl TestApp {
    fn authenticated(mut self) -> Self {
        self.sessions = Arc::new(ActiveSessions);
        self
    }

    fn notes(mut self, notes: Arc<dyn NoteUseCases>) -> Self {
        self.notes = notes;
        self
    }

    fn cookie_path(mut self, cookie_path: &str) -> Self {
        self.cookie_path = cookie_path.into();
        self
    }

    fn mcp(
        mut self,
        base_url: &'static str,
        allowed_origins: Vec<String>,
        authenticator: Arc<dyn TestMcpAccessTokens>,
    ) -> Self {
        self.mcp = Some((base_url, allowed_origins, authenticator));
        self
    }

    fn router(self) -> Router {
        let state = ApiState::new(
            self.notes,
            self.sessions,
            Arc::new(Oidc),
            self.cookie_path,
            "https://example.test".into(),
        );
        let state = match self.mcp {
            Some((base_url, allowed_origins, authenticator)) => {
                let base_url = url::Url::parse(base_url).expect("base URL");
                state.with_mcp(McpEndpoint::new(
                    Arc::new(TestMcpOAuth { authenticator }),
                    &base_url,
                    allowed_origins,
                ))
            }
            None => state,
        };
        router(state)
    }
}

fn app() -> Router {
    TestApp::default().router()
}

fn mcp_app() -> Router {
    mcp_app_with_authenticator(Arc::new(TestMcpAuthenticator))
}

fn authenticated_mcp_app() -> Router {
    TestApp::default()
        .authenticated()
        .mcp(
            "https://example.test",
            vec!["https://chatgpt.com".into()],
            Arc::new(TestMcpAuthenticator),
        )
        .router()
}

fn mcp_app_with_authenticator(authenticator: Arc<dyn TestMcpAccessTokens>) -> Router {
    TestApp::default()
        .mcp(
            "https://example.test",
            vec!["https://chatgpt.com".into()],
            authenticator,
        )
        .router()
}

fn authenticated_app() -> Router {
    TestApp::default().authenticated().router()
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
        "= 同期ノート\n:marginalis-tags: 同期, 試験\n\n本文".into(),
        vec!["同期".into(), "試験".into()],
        UnixMillis::new(1_000),
        UnixMillis::new(2_000),
        Revision::new(3).expect("revision"),
        None,
    )
    .expect("consistent note")
}

fn ui_app(notes: Vec<Note>, render_fails: bool, cookie_path: &str) -> Router {
    TestApp::default()
        .authenticated()
        .notes(Arc::new(UiNotes {
            notes,
            render_fails,
        }))
        .cookie_path(cookie_path)
        .router()
}

fn authenticated_request(uri: &str) -> Request<Body> {
    Request::get(uri)
        .header(header::COOKIE, "marginalis_session=active-session")
        .body(Body::empty())
        .expect("request")
}

fn subpath_mcp_app() -> Router {
    TestApp::default()
        .cookie_path("/marginalis")
        .mcp(
            "https://example.test/marginalis",
            vec![],
            Arc::new(TestMcpAuthenticator),
        )
        .router()
}

mod ui_contracts;

mod mcp_transport;

mod oauth;

mod rest_notes;
