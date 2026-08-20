use super::super::*;
use async_trait::async_trait;
use axum::{body::Body, http::Request};
use marginalis_application::{
    AuthenticationUseCaseError, MathMacroSettings, MathMacroUseCaseError, MathMacroUseCases,
    McpAuthenticatedActor, McpAuthorizationClient, McpClientRegistrationMethod, McpOAuthClient,
    McpOAuthUseCaseError, McpOAuthUseCases, McpResourcePolicy, McpScopeCeilingSetting,
    McpScopeCeilingUseCaseError, McpTokenPair, McpValidatedAuthorizationRequest, NoteAclChange,
    NoteAclState, NoteAdvisoryDiagnostic, NoteAdvisorySeverity, NoteGraph, NoteGraphNote,
    NoteGraphQuery, NoteListQuery, NotePreview, NoteProfile, NoteProfileAdvisoryRule,
    NoteProfileExample, NoteProfileLimits, NoteProfileNormalization, NoteProfileSyntax,
    NoteRenderContext, NoteReviewDetails, NoteSourcePosition, NoteSourceSpan, NoteSourceSpanKind,
    NoteSyncPage, NoteSyncPhase, NoteUseCaseError, NoteUseCases, NoteValidationCode,
    NoteValidationDiagnostic, NoteView, NoteWritePolicy, OidcAuthenticationUseCases, RelatedNotes,
    WebSessionUseCases,
};
use marginalis_domain::{
    Actor, AuthenticatedSession, DeletedNoteListEntry, Identity, Note, NoteAccess,
    NoteCreationSource, NoteDraft, NoteId, NoteListEntry, NoteRestore, NoteReviewTracking,
    NoteSummary, NoteValidationTarget, PrincipalId, PrincipalRef, Revision, UnixMillis,
    Utf8ByteSpan, WebSession,
};
use std::{
    io,
    sync::{Mutex, OnceLock},
};
use tracing_subscriber::fmt::MakeWriter;

// テストから使うfake実装、harness、log捕捉。テスト本体はtests.rsと配下のsubmoduleへ置く。

pub(super) fn test_identity(issuer: &str, subject: &str) -> Identity {
    Identity::new(issuer.into(), subject.into()).expect("valid identity")
}

pub(super) fn test_principal(issuer: &str, subject: &str) -> PrincipalRef {
    PrincipalRef::new(
        PrincipalId::new(1).expect("positive principal ID"),
        test_identity(issuer, subject),
    )
}

pub(super) fn test_actor(issuer: &str, subject: &str) -> Actor {
    Actor::for_single_identity(
        PrincipalId::new(1).expect("positive principal ID"),
        test_identity(issuer, subject),
    )
}

macro_rules! implement_note_use_cases {
    ($type:ty) => {
        #[async_trait]
        impl NoteUseCases for $type {
            async fn list_visible_notes(
                &self,
                actor: Actor,
                query: NoteListQuery,
            ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
                <$type>::list_visible_notes(self, actor, query).await
            }

            async fn list_note_templates(
                &self,
                actor: Actor,
            ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
                <$type>::list_note_templates(self, actor).await
            }

            async fn list_owned_deleted_notes(
                &self,
                actor: Actor,
            ) -> Result<Vec<DeletedNoteListEntry>, NoteUseCaseError> {
                <$type>::list_owned_deleted_notes(self, actor).await
            }

            async fn read_note(
                &self,
                actor: Actor,
                note_id: NoteId,
            ) -> Result<Note, NoteUseCaseError> {
                <$type>::read_note(self, actor, note_id).await
            }

            async fn read_note_outline(
                &self,
                actor: Actor,
                note_id: NoteId,
            ) -> Result<(Note, marginalis_application::NoteOutline), NoteUseCaseError> {
                <$type>::read_note_outline(self, actor, note_id).await
            }

            async fn read_note_fragment(
                &self,
                actor: Actor,
                note_id: NoteId,
                start_line: usize,
                end_line: usize,
                expected_revision: Option<Revision>,
            ) -> Result<(Note, String), NoteUseCaseError> {
                <$type>::read_note_fragment(
                    self,
                    actor,
                    note_id,
                    start_line,
                    end_line,
                    expected_revision,
                )
                .await
            }

            async fn apply_note_patch(
                &self,
                actor: Actor,
                note_id: NoteId,
                patch: &str,
                expected_revision: Revision,
                policy: NoteWritePolicy,
                dry_run: bool,
            ) -> Result<marginalis_application::NotePatchApplication, NoteUseCaseError> {
                <$type>::apply_note_patch(
                    self,
                    actor,
                    note_id,
                    patch,
                    expected_revision,
                    policy,
                    dry_run,
                )
                .await
            }

            async fn create_note(
                &self,
                actor: Actor,
                draft: NoteDraft,
                policy: NoteWritePolicy,
                created_via: NoteCreationSource,
            ) -> Result<Note, NoteUseCaseError> {
                <$type>::create_note(self, actor, draft, policy, created_via).await
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

            async fn list_note_revisions(
                &self,
                actor: Actor,
                note_id: NoteId,
            ) -> Result<Vec<marginalis_domain::NoteRevisionSummary>, NoteUseCaseError> {
                let note = <$type>::read_note(self, actor.clone(), note_id).await?;
                Ok(vec![marginalis_domain::NoteRevisionSummary {
                    revision: note.revision(),
                    changed_at: note.updated_at(),
                    changed_by: actor.principal().clone(),
                    kind: marginalis_domain::NoteRevisionKind::Imported,
                }])
            }

            async fn read_note_revision(
                &self,
                actor: Actor,
                note_id: NoteId,
                revision: Revision,
            ) -> Result<marginalis_application::NoteRevisionView, NoteUseCaseError> {
                let note = <$type>::read_note(self, actor.clone(), note_id).await?;
                if note.revision() != revision {
                    return Err(NoteUseCaseError::NotFound);
                }
                Ok(marginalis_application::NoteRevisionView {
                    revision: marginalis_domain::NoteRevisionSnapshot::new(
                        note,
                        actor.principal().clone(),
                        marginalis_domain::NoteRevisionKind::Imported,
                    ),
                    access: NoteAccess::Manage,
                })
            }

            async fn compare_note_revisions(
                &self,
                actor: Actor,
                note_id: NoteId,
                from_revision: Revision,
                to_revision: Revision,
            ) -> Result<marginalis_application::NoteRevisionDiff, NoteUseCaseError> {
                let note = <$type>::read_note(self, actor, note_id).await?;
                if note.revision() != from_revision || note.revision() != to_revision {
                    return Err(NoteUseCaseError::NotFound);
                }
                Ok(marginalis_application::NoteRevisionDiff {
                    from_revision,
                    to_revision,
                    unified_diff: String::new(),
                })
            }

            async fn restore_note_revision(
                &self,
                actor: Actor,
                note_id: NoteId,
                revision: Revision,
                expected_revision: Revision,
                _policy: NoteWritePolicy,
            ) -> Result<Note, NoteUseCaseError> {
                let note = <$type>::read_note(self, actor, note_id).await?;
                if note.revision() != revision || note.revision() != expected_revision {
                    return Err(NoteUseCaseError::Conflict);
                }
                Ok(note)
            }

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

            async fn read_note_review(
                &self,
                actor: Actor,
                note_id: NoteId,
            ) -> Result<NoteReviewDetails, NoteUseCaseError> {
                let note = <$type>::read_note(self, actor, note_id).await?;
                let last_review = note.last_review();
                Ok(NoteReviewDetails {
                    note_id,
                    current_revision: note.revision(),
                    status: note.review_status(),
                    reviewed_revision: last_review.map(|review| review.revision()),
                    reviewed_at: last_review.map(|review| review.reviewed_at()),
                    reviewer: last_review
                        .map(|review| review.reviewer().primary_identity().clone()),
                })
            }

            async fn mark_note_reviewed(
                &self,
                actor: Actor,
                note_id: NoteId,
                expected_revision: Revision,
            ) -> Result<NoteReviewDetails, NoteUseCaseError> {
                let reviewed_revision = Revision::new(expected_revision.get() + 1)
                    .map_err(|_| NoteUseCaseError::Unavailable)?;
                Ok(NoteReviewDetails {
                    note_id,
                    current_revision: reviewed_revision,
                    status: marginalis_domain::NoteReviewStatus::Reviewed,
                    reviewed_revision: Some(reviewed_revision),
                    reviewed_at: Some(UnixMillis::new(3)),
                    reviewer: Some(actor.authenticated_identity().clone()),
                })
            }

            async fn sync_notes(
                &self,
                _actor: Actor,
                _cursor: Option<String>,
                _limit: Option<usize>,
            ) -> Result<NoteSyncPage, NoteUseCaseError> {
                Ok(NoteSyncPage {
                    phase: NoteSyncPhase::Snapshot,
                    entries: Vec::new(),
                    next_cursor: "next-sync-cursor".into(),
                    has_more: false,
                    cursor_expires_at: UnixMillis::new(3_024_000_000),
                })
            }
        }
    };
}
#[derive(Clone, Default)]
pub(super) struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl CapturedLogs {
    pub(super) fn clear(&self) {
        self.0.lock().expect("captured logs").clear();
    }

    pub(super) fn text(&self) -> String {
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

pub(super) fn global_captured_logs() -> CapturedLogs {
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

pub(super) fn assert_log_line(logs: &str, expected_fields: &[&str]) {
    assert!(
        logs.lines()
            .any(|line| expected_fields.iter().all(|field| line.contains(field))),
        "次のfieldを同じログ行で確認できませんでした: {expected_fields:?}\n{logs}"
    );
}

pub(super) struct Notes;

fn test_source_position(source: &str, byte_offset: usize) -> NoteSourcePosition {
    let prefix = &source[..byte_offset];
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    NoteSourcePosition {
        line: u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count() + 1)
            .expect("test line"),
        column: u32::try_from(prefix[line_start..].encode_utf16().count() + 1)
            .expect("test column"),
    }
}

/// 題名行だけを写した、決定的なspan注釈を返す。
pub(super) fn test_spans(source: &str) -> Vec<NoteSourceSpan> {
    if !source.starts_with("= ") {
        return Vec::new();
    }
    let line_end = source.find('\n').unwrap_or(source.len()) as u32;
    vec![NoteSourceSpan {
        kind: NoteSourceSpanKind::DocumentTitle,
        span: Utf8ByteSpan {
            start: 0,
            end: line_end,
        },
        content_span: Some(Utf8ByteSpan {
            start: 2,
            end: line_end,
        }),
        marker_spans: vec![Utf8ByteSpan { start: 0, end: 2 }],
        level: None,
    }]
}

pub(super) fn test_advisories(source: &str) -> Vec<NoteAdvisoryDiagnostic> {
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
                position: Some(test_source_position(source, start)),
                message: "a space is required before the inline macro".into(),
            },
            NoteAdvisoryDiagnostic {
                code: "document-information".into(),
                severity: NoteAdvisorySeverity::Information,
                target: NoteValidationTarget::Source,
                span: None,
                position: None,
                message: "document information".into(),
            },
            NoteAdvisoryDiagnostic {
                code: "document-hint".into(),
                severity: NoteAdvisorySeverity::Hint,
                target: NoteValidationTarget::Source,
                span: None,
                position: None,
                message: "document hint".into(),
            },
        ]
    })
}

pub(super) fn reject_test_warnings(
    source: &str,
    policy: NoteWritePolicy,
) -> Result<(), NoteUseCaseError> {
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
    pub(super) async fn list_visible_notes(
        &self,
        _actor: Actor,
        _query: NoteListQuery,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        let note = mcp_note();
        Ok(vec![NoteListEntry {
            summary: NoteSummary::from(&note),
            access: NoteAccess::Manage,
        }])
    }

    pub(super) async fn list_note_templates(
        &self,
        _actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        let note = mcp_note();
        let mut summary = NoteSummary::from(&note);
        summary.title = "実験記録の雛形".into();
        summary.tags = vec!["テンプレート".into()];
        Ok(vec![NoteListEntry {
            summary,
            access: NoteAccess::Manage,
        }])
    }

    pub(super) async fn list_owned_deleted_notes(
        &self,
        _actor: Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, NoteUseCaseError> {
        Ok(vec![DeletedNoteListEntry {
            note_id: mcp_note().note_id(),
            title: "削除済みノート".into(),
            deleted_at: UnixMillis::new(100),
            purge_at: UnixMillis::new(200),
            revision: Revision::new(2).expect("revision"),
        }])
    }

    pub(super) async fn read_note(
        &self,
        _actor: Actor,
        note_id: NoteId,
    ) -> Result<Note, NoteUseCaseError> {
        let note = mcp_note();
        if note.note_id() == note_id {
            Ok(note)
        } else {
            Err(NoteUseCaseError::NotFound)
        }
    }

    /// 実物のoutlineはAsciiDoc解析を要するため、行数だけを計算した空の構成を返す。
    pub(super) async fn read_note_outline(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<(Note, marginalis_application::NoteOutline), NoteUseCaseError> {
        let note = Notes::read_note(self, actor, note_id).await?;
        let line_count = note.source().lines().count();
        Ok((
            note,
            marginalis_application::NoteOutline {
                sections: Vec::new(),
                line_count,
            },
        ))
    }

    pub(super) async fn read_note_fragment(
        &self,
        actor: Actor,
        note_id: NoteId,
        start_line: usize,
        end_line: usize,
        expected_revision: Option<Revision>,
    ) -> Result<(Note, String), NoteUseCaseError> {
        let note = Notes::read_note(self, actor, note_id).await?;
        if expected_revision.is_some_and(|expected| note.revision() != expected) {
            return Err(NoteUseCaseError::Conflict);
        }
        let lines: Vec<&str> = note.source().lines().collect();
        if start_line == 0 || end_line < start_line || end_line > lines.len() {
            return Err(NoteUseCaseError::InvalidLineRange);
        }
        let fragment = format!("{}\n", lines[start_line - 1..end_line].join("\n"));
        Ok((note, fragment))
    }

    /// patchの解釈と適用は本物の実装を使い、保存だけをfakeで置き換える。
    pub(super) async fn apply_note_patch(
        &self,
        actor: Actor,
        note_id: NoteId,
        patch: &str,
        expected_revision: Revision,
        _policy: NoteWritePolicy,
        dry_run: bool,
    ) -> Result<marginalis_application::NotePatchApplication, NoteUseCaseError> {
        let note = Notes::read_note(self, actor, note_id).await?;
        if note.revision() != expected_revision {
            return Err(NoteUseCaseError::Conflict);
        }
        let outcome = marginalis_application::apply_note_patch(note.source(), patch)
            .map_err(NoteUseCaseError::PatchRejected)?;
        Ok(marginalis_application::NotePatchApplication {
            note: (!dry_run).then(|| note.clone()),
            hunks_applied: outcome.hunks_applied,
            lines_added: outcome.lines_added,
            lines_removed: outcome.lines_removed,
            advisories: Vec::new(),
        })
    }

    pub(super) async fn create_note(
        &self,
        _actor: Actor,
        draft: NoteDraft,
        policy: NoteWritePolicy,
        _created_via: NoteCreationSource,
    ) -> Result<Note, NoteUseCaseError> {
        if !draft.source.starts_with("= ") {
            return Err(NoteUseCaseError::Validation(vec![
                NoteValidationDiagnostic {
                    code: NoteValidationCode::InvalidTitle.as_str().into(),
                    target: NoteValidationTarget::Source,
                    span: None,
                    position: Some(NoteSourcePosition { line: 1, column: 1 }),
                    message: "title is invalid".into(),
                },
                NoteValidationDiagnostic {
                    code: NoteValidationCode::UnsupportedSourceLanguage
                        .as_str()
                        .into(),
                    target: NoteValidationTarget::Source,
                    span: Some(Utf8ByteSpan { start: 8, end: 13 }),
                    position: Some(NoteSourcePosition { line: 1, column: 9 }),
                    message: "source language is not allowed".into(),
                },
            ]));
        }
        reject_test_warnings(&draft.source, policy)?;
        Err(NoteUseCaseError::Unavailable)
    }

    pub(super) async fn update_note(
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

    pub(super) async fn preview_new_note(
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
                    position: Some(NoteSourcePosition { line: 1, column: 1 }),
                    message: "title is invalid".into(),
                },
            ]))
        } else {
            let diagnostics = test_advisories(&draft.source);
            Ok(NotePreview {
                html: "<article><p>プレビュー</p></article>".into(),
                math_macros: Vec::new(),
                diagnostics,
                spans: test_spans(&draft.source),
            })
        }
    }

    pub(super) async fn preview_note_update(
        &self,
        actor: Actor,
        _note_id: NoteId,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        self.preview_new_note(actor, draft, context).await
    }

    pub(super) async fn soft_delete_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    pub(super) async fn restore_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        if expected_revision.get() == 99 {
            Err(NoteUseCaseError::RetentionExpired)
        } else {
            Err(NoteUseCaseError::Unavailable)
        }
    }

    pub(super) fn export_note_source(&self, _note: &Note) -> Result<String, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    pub(super) async fn read_note_view(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _context: NoteRenderContext,
    ) -> Result<NoteView, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    pub(super) async fn read_note_graph(
        &self,
        _actor: Actor,
        _query: NoteGraphQuery,
    ) -> Result<NoteGraph, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    pub(super) async fn read_note_acl(
        &self,
        _actor: Actor,
        _note_id: NoteId,
    ) -> Result<NoteAclState, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    pub(super) async fn replace_note_acl(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _entries: Vec<NoteAclChange>,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    pub(super) fn note_profile(&self) -> NoteProfile {
        const BIBLIOGRAPHY_GUIDANCE: &str = "Use bibliographic metadata supplied by the user or an identified source. Never invent or infer authors, titles, publication years, DOIs, or other bibliographic metadata.";
        const BIBLIOGRAPHY_EXAMPLE: &str = "= 先行研究の整理\n:marginalis-tags: 文献, 研究\n\nSmithらは、対象の手法が有効だと報告しています <<smith2024>>。\n\n[bibliography]\n== 参考文献\n\n* [[[smith2024]]] Smith, A. et al. _Example Paper_. Example Journal, 2024. https://doi.org/10.1234/replace-with-doi[DOI]";
        NoteProfile {
            profile_version: 6,
            adocweave_package_version: "0.23.0",
            limits: NoteProfileLimits {
                max_title_characters: 200,
                max_source_bytes: 524_288,
                max_patch_bytes: 524_288,
                max_patch_hunks: 100,
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
            advisory_rules: vec![NoteProfileAdvisoryRule {
                code: "macro-boundary",
                description: "インラインマクロ境界の不足",
                severity: NoteAdvisorySeverity::Warning,
            }],
            examples: vec![NoteProfileExample {
                kind: "bibliography",
                description: "Complete document with a bibliography entry and an in-text reference",
                body: BIBLIOGRAPHY_EXAMPLE,
            }],
        }
    }
}

pub(super) struct UiNotes {
    pub(super) notes: Vec<Note>,
    pub(super) render_fails: bool,
    pub(super) creation_sources: Mutex<Vec<NoteCreationSource>>,
    pub(super) list_queries: Mutex<Vec<NoteListQuery>>,
}

impl UiNotes {
    // Web UIの試験は部分取得とpatchを使わないため、対象なしとして拒否する。
    pub(super) async fn read_note_outline(
        &self,
        _actor: Actor,
        _note_id: NoteId,
    ) -> Result<(Note, marginalis_application::NoteOutline), NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    pub(super) async fn read_note_fragment(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _start_line: usize,
        _end_line: usize,
        _expected_revision: Option<Revision>,
    ) -> Result<(Note, String), NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    pub(super) async fn apply_note_patch(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _patch: &str,
        _expected_revision: Revision,
        _policy: NoteWritePolicy,
        _dry_run: bool,
    ) -> Result<marginalis_application::NotePatchApplication, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    pub(super) async fn list_note_templates(
        &self,
        _actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        Ok(self
            .notes
            .iter()
            .filter(|note| note.tags().iter().any(|tag| tag == "テンプレート"))
            .map(|note| NoteListEntry {
                summary: NoteSummary::from(note),
                access: NoteAccess::Edit,
            })
            .collect())
    }

    pub(super) async fn list_visible_notes(
        &self,
        _actor: Actor,
        query: NoteListQuery,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        self.list_queries
            .lock()
            .expect("list query lock")
            .push(query);
        Ok(self
            .notes
            .iter()
            .map(|note| NoteListEntry {
                summary: NoteSummary::from(note),
                access: NoteAccess::Edit,
            })
            .collect())
    }

    pub(super) async fn list_owned_deleted_notes(
        &self,
        _actor: Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, NoteUseCaseError> {
        Ok(Vec::new())
    }

    pub(super) async fn read_note(
        &self,
        _actor: Actor,
        note_id: NoteId,
    ) -> Result<Note, NoteUseCaseError> {
        self.notes
            .iter()
            .find(|note| note.note_id() == note_id)
            .cloned()
            .ok_or(NoteUseCaseError::NotFound)
    }

    pub(super) async fn create_note(
        &self,
        _actor: Actor,
        _draft: NoteDraft,
        _policy: NoteWritePolicy,
        created_via: NoteCreationSource,
    ) -> Result<Note, NoteUseCaseError> {
        self.creation_sources
            .lock()
            .expect("creation source lock")
            .push(created_via);
        Err(NoteUseCaseError::Unavailable)
    }

    pub(super) async fn update_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _draft: NoteDraft,
        _expected_revision: Revision,
        _policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    pub(super) async fn preview_new_note(
        &self,
        _actor: Actor,
        _draft: NoteDraft,
        _context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    pub(super) async fn preview_note_update(
        &self,
        actor: Actor,
        _note_id: NoteId,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        self.preview_new_note(actor, draft, context).await
    }

    pub(super) async fn soft_delete_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    pub(super) async fn restore_note(
        &self,
        _actor: Actor,
        _note_id: NoteId,
        _expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    pub(super) fn export_note_source(&self, _note: &Note) -> Result<String, NoteUseCaseError> {
        Err(NoteUseCaseError::Unavailable)
    }

    pub(super) async fn read_note_view(
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
                math_macros: Vec::new(),
                related: RelatedNotes {
                    outgoing: related.clone(),
                    incoming: related,
                },
            })
        }
    }

    pub(super) async fn read_note_acl(
        &self,
        _actor: Actor,
        _note_id: NoteId,
    ) -> Result<NoteAclState, NoteUseCaseError> {
        Err(NoteUseCaseError::NotFound)
    }

    pub(super) async fn replace_note_acl(
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
    pub(super) async fn read_note_graph(
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

    pub(super) fn note_profile(&self) -> NoteProfile {
        Notes.note_profile()
    }
}

implement_note_use_cases!(Notes);
implement_note_use_cases!(UiNotes);

pub(super) struct Sessions;

pub(super) struct MathMacros;

#[async_trait]
impl MathMacroUseCases for MathMacros {
    async fn read_math_macros(
        &self,
        _actor: Actor,
    ) -> Result<MathMacroSettings, MathMacroUseCaseError> {
        Ok(MathMacroSettings::default())
    }

    async fn replace_math_macros(
        &self,
        _actor: Actor,
        macros: Vec<marginalis_application::MathMacro>,
        expected_revision: i64,
    ) -> Result<MathMacroSettings, MathMacroUseCaseError> {
        Ok(MathMacroSettings {
            macros,
            revision: expected_revision + 1,
        })
    }
}

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

pub(super) struct ActiveSessions;

#[async_trait]
impl WebSessionUseCases for ActiveSessions {
    async fn authenticate_session(
        &self,
        session_id: String,
    ) -> Result<Option<AuthenticatedSession>, AuthenticationUseCaseError> {
        Ok(
            (session_id == "active-session").then(|| AuthenticatedSession {
                actor: test_actor("https://id.example.test", "alice"),
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

pub(super) struct Oidc;

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
pub(super) trait TestMcpAccessTokens: Send + Sync {
    async fn authenticate_access_token(
        &self,
        token: String,
        resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError>;
}

pub(super) struct TestMcpAuthenticator;

#[async_trait]
impl TestMcpAccessTokens for TestMcpAuthenticator {
    async fn authenticate_access_token(
        &self,
        token: String,
        resource_uri: String,
    ) -> Result<Option<McpAuthenticatedActor>, McpOAuthUseCaseError> {
        Ok((matches!(
            token.as_str(),
            "external-token"
                | "valid-token"
                | "read-token"
                | "write-token"
                | "bibliography-read-token"
                | "sync-token"
        ) && resource_uri.ends_with("/mcp"))
        .then(|| McpAuthenticatedActor {
            actor: test_actor("https://kanidm.example.test", "alice"),
            scopes: match token.as_str() {
                "read-token" | "external-token" => vec!["notes:read".into()],
                "write-token" => vec!["notes:write".into()],
                "bibliography-read-token" => vec!["bibliography:read".into()],
                "sync-token" => vec!["notes:sync".into()],
                _ => vec![
                    "notes:read".into(),
                    "notes:write".into(),
                    "notes:delete".into(),
                    "bibliography:read".into(),
                    "bibliography:write".into(),
                    "bibliography:delete".into(),
                ],
            },
        }))
    }
}

pub(super) struct UnavailableMcpAuthenticator;

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

pub(super) struct TestMcpOAuth {
    authenticator: Arc<dyn TestMcpAccessTokens>,
    pub(super) resource_policy: McpResourcePolicy,
}

#[async_trait]
impl McpOAuthUseCases for TestMcpOAuth {
    fn resource_policy(&self) -> McpResourcePolicy {
        self.resource_policy.clone()
    }

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
        let display_name = match client_id.as_str() {
            "long-client" => "非常に長いクライアント名".repeat(24),
            value if value.contains('<') => {
                "危険 <script>alert('x')</script> & クライアント".into()
            }
            _ => "Test MCP client".into(),
        };
        Ok(McpAuthorizationClient {
            client: McpOAuthClient {
                client_id,
                display_name,
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
        if !matches!(
            request.resource_uri.as_str(),
            "https://example.test/mcp" | "https://example.test/marginalis/mcp"
        ) {
            return Err(McpOAuthUseCaseError::InvalidTarget);
        }
        Ok(McpValidatedAuthorizationRequest {
            client: resolved.client,
            registration_method: resolved.registration_method,
            redirect_uri: if request.redirect_uri.is_some() {
                marginalis_application::McpResolvedRedirectUri::Supplied(resolved.redirect_uri)
            } else {
                marginalis_application::McpResolvedRedirectUri::Inferred(resolved.redirect_uri)
            },
            resource_uri: request.resource_uri,
            scopes: request.scopes,
            code_challenge: request.code_challenge,
        })
    }

    /// `ceiling-client`はclient別上限で``notes:read``だけを許可する構成を表す。
    async fn grantable_scopes(
        &self,
        _actor: Actor,
        client_id: String,
        requested: Vec<String>,
    ) -> Result<Vec<String>, McpOAuthUseCaseError> {
        if client_id == "unavailable-ceiling-client" {
            return Err(McpOAuthUseCaseError::Unavailable);
        }
        if client_id == "ceiling-client" {
            return Ok(requested
                .into_iter()
                .filter(|scope| scope == "notes:read")
                .collect());
        }
        Ok(requested)
    }

    async fn authorize(
        &self,
        _actor: Actor,
        request: McpValidatedAuthorizationRequest,
    ) -> Result<String, McpOAuthUseCaseError> {
        if request.client.client_id == "consent-client"
            && request.scopes == ["notes:read".to_owned()]
        {
            Ok("test-authorization-code".into())
        } else {
            Err(McpOAuthUseCaseError::InvalidRequest)
        }
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

    async fn principal_scope_ceiling(
        &self,
        _actor: Actor,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingUseCaseError> {
        Ok(McpScopeCeilingSetting {
            scopes: vec!["notes:read".into(), "notes:write".into()],
            revision: 2,
        })
    }

    async fn client_authorizations(
        &self,
        _actor: Actor,
    ) -> Result<Vec<marginalis_application::McpClientAuthorization>, McpScopeCeilingUseCaseError>
    {
        Ok(vec![marginalis_application::McpClientAuthorization {
            client_id: "consent-client".into(),
            display_name: "Consent client".into(),
            registration_method: McpClientRegistrationMethod::Dynamic,
            granted_scopes: vec!["notes:read".into(), "notes:write".into()],
            scope_ceiling: marginalis_application::McpEffectiveScopeCeiling {
                configured: false,
                setting: McpScopeCeilingSetting {
                    scopes: vec![
                        "notes:read".into(),
                        "notes:write".into(),
                        "notes:delete".into(),
                        "notes:sync".into(),
                        "bibliography:read".into(),
                        "bibliography:write".into(),
                        "bibliography:delete".into(),
                    ],
                    revision: 0,
                },
            },
            authorized_at: marginalis_domain::UnixMillis::new(1_000),
            last_used_at: Some(marginalis_domain::UnixMillis::new(2_000)),
            active: true,
        }])
    }

    async fn replace_principal_scope_ceiling(
        &self,
        _actor: Actor,
        scopes: Vec<String>,
        expected_revision: i64,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingUseCaseError> {
        if expected_revision != 2 {
            return Err(McpScopeCeilingUseCaseError::Conflict);
        }
        Ok(McpScopeCeilingSetting {
            scopes,
            revision: 3,
        })
    }

    async fn replace_client_scope_ceiling(
        &self,
        _actor: Actor,
        _client_id: String,
        scopes: Vec<String>,
        expected_revision: i64,
    ) -> Result<McpScopeCeilingSetting, McpScopeCeilingUseCaseError> {
        Ok(McpScopeCeilingSetting {
            scopes,
            revision: expected_revision + 1,
        })
    }

    async fn delete_client_scope_ceiling(
        &self,
        _actor: Actor,
        client_id: String,
        expected_revision: i64,
    ) -> Result<(), McpScopeCeilingUseCaseError> {
        if client_id == "unknown-client" {
            return Err(McpScopeCeilingUseCaseError::ClientNotFound);
        }
        if expected_revision <= 0 {
            return Err(McpScopeCeilingUseCaseError::Invalid);
        }
        if expected_revision != 1 {
            return Err(McpScopeCeilingUseCaseError::Conflict);
        }
        Ok(())
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
pub(super) struct TestApp {
    pub(super) notes: Arc<dyn NoteUseCases>,
    pub(super) bibliography: Option<Arc<marginalis_application::BibliographyApplication>>,
    pub(super) bibliography_import:
        Option<Arc<dyn marginalis_application::BibliographyImportUseCases>>,
    pub(super) sessions: Arc<dyn WebSessionUseCases>,
    pub(super) cookie_path: String,
    mcp: Option<(&'static str, Vec<String>, Arc<dyn TestMcpAccessTokens>)>,
}

impl Default for TestApp {
    fn default() -> Self {
        Self {
            notes: Arc::new(Notes),
            bibliography: None,
            bibliography_import: None,
            sessions: Arc::new(Sessions),
            cookie_path: "/".into(),
            mcp: None,
        }
    }
}

impl TestApp {
    pub(super) fn authenticated(mut self) -> Self {
        self.sessions = Arc::new(ActiveSessions);
        self
    }

    pub(super) fn notes(mut self, notes: Arc<dyn NoteUseCases>) -> Self {
        self.notes = notes;
        self
    }

    pub(super) fn bibliography_import(
        mut self,
        bibliography_import: Arc<dyn marginalis_application::BibliographyImportUseCases>,
    ) -> Self {
        self.bibliography_import = Some(bibliography_import);
        self
    }

    pub(super) fn bibliography(
        mut self,
        bibliography: Arc<marginalis_application::BibliographyApplication>,
    ) -> Self {
        self.bibliography = Some(bibliography);
        self
    }

    pub(super) fn cookie_path(mut self, cookie_path: &str) -> Self {
        self.cookie_path = cookie_path.into();
        self
    }

    pub(super) fn mcp(
        mut self,
        base_url: &'static str,
        allowed_origins: Vec<String>,
        authenticator: Arc<dyn TestMcpAccessTokens>,
    ) -> Self {
        self.mcp = Some((base_url, allowed_origins, authenticator));
        self
    }

    pub(super) fn router(self) -> Router {
        let state = ApiState::new(
            self.notes,
            Arc::new(MathMacros),
            self.sessions,
            Arc::new(Oidc),
            self.cookie_path,
            "https://example.test".into(),
        );
        let state = match self.bibliography_import {
            Some(bibliography_import) => state.with_bibliography_import(bibliography_import),
            None => state,
        };
        let state = match self.bibliography {
            Some(bibliography) => state.with_bibliography(bibliography),
            None => state,
        };
        let state = match self.mcp {
            Some((base_url, allowed_origins, authenticator)) => {
                let base_url = url::Url::parse(base_url).expect("base URL");
                let resource_policy = marginalis_application::McpResourcePolicy::new(
                    McpEndpoint::resource_uri_for(&base_url),
                    "Marginalis MCP".into(),
                    vec![
                        "notes:read".into(),
                        "notes:write".into(),
                        "notes:delete".into(),
                        "notes:sync".into(),
                        "bibliography:read".into(),
                        "bibliography:write".into(),
                        "bibliography:delete".into(),
                    ],
                    vec!["notes:read".into()],
                )
                .expect("MCP resource policy");
                state.with_mcp(
                    McpEndpoint::new(
                        Arc::new(TestMcpOAuth {
                            authenticator,
                            resource_policy,
                        }),
                        &base_url,
                        allowed_origins,
                    )
                    .expect("MCP endpoint"),
                )
            }
            None => state,
        };
        router(state)
    }
}

pub(super) fn app() -> Router {
    TestApp::default().router()
}

pub(super) fn mcp_app() -> Router {
    mcp_app_with_authenticator(Arc::new(TestMcpAuthenticator))
}

pub(super) fn authenticated_mcp_app() -> Router {
    TestApp::default()
        .authenticated()
        .mcp(
            "https://example.test",
            vec!["https://chatgpt.com".into()],
            Arc::new(TestMcpAuthenticator),
        )
        .router()
}

pub(super) fn mcp_app_with_authenticator(authenticator: Arc<dyn TestMcpAccessTokens>) -> Router {
    TestApp::default()
        .mcp(
            "https://example.test",
            vec!["https://chatgpt.com".into()],
            authenticator,
        )
        .router()
}

pub(super) fn authenticated_app() -> Router {
    TestApp::default().authenticated().router()
}

pub(super) fn ui_note(title: &str) -> Note {
    Note::restore(NoteRestore {
        note_id: NoteId::new(
            "0197c9bc-0000-7000-8000-000000000001"
                .parse()
                .expect("note ID"),
        ),
        owner: test_principal("https://id.example.test", "alice"),
        draft: NoteDraft {
            title: title.into(),
            source: "本文".into(),
            tags: vec!["試験".into()],
        },
        created_at: UnixMillis::new(1),
        updated_at: UnixMillis::new(2),
        revision: Revision::INITIAL,
        deleted_at: None,
        created_via: NoteCreationSource::Web,
        review: NoteReviewTracking::pending(),
    })
    .expect("consistent note")
}

pub(super) fn mcp_note() -> Note {
    Note::restore(NoteRestore {
        note_id: NoteId::new(
            "0197c9bc-0000-7000-8000-000000000002"
                .parse()
                .expect("note ID"),
        ),
        owner: test_principal("https://id.example.test", "alice"),
        draft: NoteDraft {
            title: "同期ノート".into(),
            source: "= 同期ノート\n:marginalis-tags: 同期, 試験\n\n本文".into(),
            tags: vec!["同期".into(), "試験".into()],
        },
        created_at: UnixMillis::new(1_000),
        updated_at: UnixMillis::new(2_000),
        revision: Revision::new(3).expect("revision"),
        deleted_at: None,
        created_via: NoteCreationSource::Mcp,
        review: NoteReviewTracking::pending(),
    })
    .expect("consistent note")
}

pub(super) fn ui_app(notes: Vec<Note>, render_fails: bool, cookie_path: &str) -> Router {
    TestApp::default()
        .authenticated()
        .notes(Arc::new(UiNotes {
            notes,
            render_fails,
            creation_sources: Mutex::new(Vec::new()),
            list_queries: Mutex::new(Vec::new()),
        }))
        .cookie_path(cookie_path)
        .router()
}

pub(super) fn authenticated_request(uri: &str) -> Request<Body> {
    // cookie名はcookie pathで変わるため、rootとサブパスの両方の名前を送る。
    Request::get(uri)
        .header(
            header::COOKIE,
            "__Host-marginalis_session=active-session; __Secure-marginalis_session=active-session",
        )
        .body(Body::empty())
        .expect("request")
}

pub(super) fn subpath_mcp_app() -> Router {
    TestApp::default()
        .cookie_path("/marginalis")
        .mcp(
            "https://example.test/marginalis",
            vec![],
            Arc::new(TestMcpAuthenticator),
        )
        .router()
}
