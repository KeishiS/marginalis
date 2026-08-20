//! ノートの作成、更新、論理削除、復元。

use marginalis_domain::{Actor, Note, NoteAccess, NoteCreationSource, NoteDraft, NoteId, Revision};

use crate::{NoteAdvisoryDiagnostic, NoteUseCaseError, NoteWritePolicy, ValidatedNoteDraft};

use super::{
    NoteApplication, NoteLinks, attachment_ids, attachments::rejected_attachment_references,
    cited_keys, patch, reference_targets,
};

/// patch適用の結果。応答へ載せる変更量と、warning未満の診断を含む。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotePatchApplication {
    /// 保存後のノート。dry runでは保存しないため`None`。
    pub note: Option<Note>,
    pub hunks_applied: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
    /// 保存を拒否しない診断。dry runでも同じ検証で得る。
    pub advisories: Vec<NoteAdvisoryDiagnostic>,
}

impl NoteApplication {
    pub async fn create_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        policy: NoteWritePolicy,
        created_via: NoteCreationSource,
    ) -> Result<Note, NoteUseCaseError> {
        let validated = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        let ValidatedNoteDraft {
            draft,
            mut diagnostics,
            reference_queries,
            citation_queries,
            attachment_queries,
            citation_style,
            source_spans: _,
        } = validated;
        if !attachment_queries.is_empty() {
            return Err(rejected_attachment_references(&attachment_queries));
        }
        // 新規作成では操作している利用者がそのまま作成者になるため、閲覧時の解決先と一致する。
        diagnostics.extend(
            self.citation_resolutions(actor.principal(), &citation_queries, citation_style)
                .await?
                .diagnostics,
        );
        reject_warnings(policy, &diagnostics)?;
        let now = self.clock.now();
        let note = Note::create(
            NoteId::new(self.random.uuid_v7()),
            actor.principal(),
            draft,
            now,
            created_via,
        );
        let reference_targets = reference_targets(&reference_queries);
        let cited_keys = cited_keys(&citation_queries);
        let attachment_ids = attachment_ids(&attachment_queries);
        self.commands
            .create_note(
                &note,
                NoteLinks {
                    reference_targets: &reference_targets,
                    cited_keys: &cited_keys,
                    attachment_ids: &attachment_ids,
                },
            )
            .await
            .map_err(NoteUseCaseError::from)?;
        Ok(note)
    }

    pub async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: Revision,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        let validated = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        let ValidatedNoteDraft {
            draft,
            mut diagnostics,
            reference_queries,
            citation_queries,
            attachment_queries,
            citation_style,
            source_spans: _,
        } = validated;
        self.validate_note_attachment_references(&actor, note_id, &attachment_queries)
            .await?;
        if !citation_queries.is_empty() {
            // 引用は閲覧時に作成者のライブラリで解決する。共有されたノートを別の利用者が
            // 更新する場合も同じ基準で判定しないと、保存できた引用が表示では解決されない。
            let owner = self
                .read_visible_note(&actor, note_id)
                .await?
                .owner()
                .clone();
            diagnostics.extend(
                self.citation_resolutions(&owner, &citation_queries, citation_style)
                    .await?
                    .diagnostics,
            );
        }
        reject_warnings(policy, &diagnostics)?;
        let reference_targets = reference_targets(&reference_queries);
        let cited_keys = cited_keys(&citation_queries);
        let attachment_ids = attachment_ids(&attachment_queries);
        self.commands
            .update_visible_note(
                &actor,
                note_id,
                expected_revision,
                &draft,
                NoteLinks {
                    reference_targets: &reference_targets,
                    cited_keys: &cited_keys,
                    attachment_ids: &attachment_ids,
                },
                self.clock.now(),
            )
            .await
            .map_err(NoteUseCaseError::from)
    }

    /// 保存済み原文へUnified Diffを厳密に適用し、update_noteと同じ全文検証を経て保存する。
    ///
    /// 手順は、認可済み読取(Edit以上)→`expected_revision`の一致確認→patchの厳密適用→
    /// 全文検証→保存の順。dry runでは検証まで行い、保存せずrevisionも増やさない。
    /// 保存はupdate_noteと同じ条件付き更新を通すため、読取後に他の更新が入っても
    /// repositoryのtransactionがrevisionを再検査して競合を拒否する。
    pub async fn apply_note_patch(
        &self,
        actor: Actor,
        note_id: NoteId,
        patch_text: &str,
        expected_revision: Revision,
        policy: NoteWritePolicy,
        dry_run: bool,
    ) -> Result<NotePatchApplication, NoteUseCaseError> {
        let accessible = self
            .queries
            .accessible_note(&actor, note_id)
            .await
            .map_err(NoteUseCaseError::from)?
            .ok_or(NoteUseCaseError::NotFound)?;
        // 書き込みには編集権限が要る。閲覧だけの利用者にも保存経路と同じ分類で拒否する。
        if accessible.access < NoteAccess::Edit {
            return Err(NoteUseCaseError::NotFound);
        }
        let note = accessible.note;
        if note.revision() != expected_revision {
            return Err(NoteUseCaseError::Conflict);
        }
        let outcome = patch::apply_note_patch(note.source(), patch_text)
            .map_err(NoteUseCaseError::PatchRejected)?;
        let validated = self
            .content
            .validate_draft(NoteDraft {
                source: outcome.source,
                title: String::new(),
                tags: Vec::new(),
            })
            .map_err(NoteUseCaseError::Validation)?;
        let ValidatedNoteDraft {
            draft,
            mut diagnostics,
            reference_queries,
            citation_queries,
            attachment_queries,
            citation_style,
            source_spans: _,
        } = validated;
        self.validate_note_attachment_references(&actor, note_id, &attachment_queries)
            .await?;
        if !citation_queries.is_empty() {
            // update_noteと同じく、引用は閲覧時の解決先である所有者のライブラリで判定する。
            diagnostics.extend(
                self.citation_resolutions(note.owner(), &citation_queries, citation_style)
                    .await?
                    .diagnostics,
            );
        }
        reject_warnings(policy, &diagnostics)?;
        if dry_run {
            return Ok(NotePatchApplication {
                note: None,
                hunks_applied: outcome.hunks_applied,
                lines_added: outcome.lines_added,
                lines_removed: outcome.lines_removed,
                advisories: diagnostics,
            });
        }
        let reference_targets = reference_targets(&reference_queries);
        let cited_keys = cited_keys(&citation_queries);
        let attachment_ids = attachment_ids(&attachment_queries);
        let saved = self
            .commands
            .update_visible_note(
                &actor,
                note_id,
                expected_revision,
                &draft,
                NoteLinks {
                    reference_targets: &reference_targets,
                    cited_keys: &cited_keys,
                    attachment_ids: &attachment_ids,
                },
                self.clock.now(),
            )
            .await
            .map_err(NoteUseCaseError::from)?;
        Ok(NotePatchApplication {
            note: Some(saved),
            hunks_applied: outcome.hunks_applied,
            lines_added: outcome.lines_added,
            lines_removed: outcome.lines_removed,
            advisories: diagnostics,
        })
    }

    pub async fn soft_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        self.commands
            .soft_delete_visible_note(&actor, note_id, expected_revision, self.clock.now())
            .await
            .map_err(NoteUseCaseError::from)
    }

    pub async fn restore_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        self.commands
            .restore_owned_deleted_note(&actor, note_id, expected_revision, self.clock.now())
            .await
            .map_err(NoteUseCaseError::from)
    }
}

pub(super) fn reject_warnings(
    policy: NoteWritePolicy,
    diagnostics: &[crate::NoteAdvisoryDiagnostic],
) -> Result<(), NoteUseCaseError> {
    if policy == NoteWritePolicy::RejectWarnings
        && diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::NoteAdvisorySeverity::Warning)
    {
        return Err(NoteUseCaseError::AdvisoriesRejected(diagnostics.to_vec()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::Ordering};

    use marginalis_domain::{
        EntityId, Note, NoteCreationSource, NoteDraft, NoteId, NoteRestore, NoteReviewTracking,
        Revision, UnixMillis,
    };

    use crate::{NoteAdvisoryDiagnostic, NoteAdvisorySeverity, NoteValidationTarget};

    use super::*;
    use crate::notes::test_support::{
        AcceptContent, CitingContent, EmptyLibrary, MemoryNotes, NoMathMacros, OneItemLibrary,
        OwnerMathMacros, actor, note_application,
    };

    #[tokio::test]
    async fn creates_a_note_using_only_application_ports() {
        let repository = Arc::new(MemoryNotes::default());
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor = actor("alice", 1);

        let created = application
            .create_note(
                actor.clone(),
                NoteDraft {
                    source: "= Portで作成\n:marginalis-tags: 設計\n\n本文".into(),
                    title: "Portで作成".into(),
                    tags: vec!["設計".into()],
                },
                NoteWritePolicy::AllowAdvisories,
                NoteCreationSource::Rest,
            )
            .await
            .expect("create note");

        assert_eq!(created.owner().primary_identity().subject(), "alice");
        assert_eq!(created.revision().get(), 1);
        assert_eq!(created.created_via(), NoteCreationSource::Rest);
        assert_eq!(
            created.review_status(),
            marginalis_domain::NoteReviewStatus::Pending
        );
        assert_eq!(
            application
                .read_note(actor, created.note_id())
                .await
                .expect("read created note"),
            created
        );
        assert_eq!(repository.notes.lock().expect("notes lock").len(), 1);
    }

    #[tokio::test]
    async fn shared_note_updates_resolve_citations_for_the_owner() {
        let repository = Arc::new(MemoryNotes::default());
        let note_id = NoteId::new(
            "0197c9bc-0000-7000-8000-000000000031"
                .parse::<EntityId>()
                .expect("UUIDv7"),
        );
        repository.notes.lock().expect("notes lock").push(
            Note::restore(NoteRestore {
                note_id,
                owner: OneItemLibrary::owner(),
                draft: NoteDraft {
                    title: "共有されたノート".into(),
                    source: "= 共有されたノート\n\n本文".into(),
                    tags: Vec::new(),
                },
                created_at: UnixMillis::new(0),
                updated_at: UnixMillis::new(1),
                revision: Revision::INITIAL,
                deleted_at: None,
                created_via: NoteCreationSource::Mcp,
                review: NoteReviewTracking::pending(),
            })
            .expect("stored note"),
        );
        let application = note_application(
            &repository,
            Arc::new(CitingContent {
                keys: vec!["smith2024".into()],
            }),
            Arc::new(OneItemLibrary),
            Arc::new(OwnerMathMacros),
        );
        let editor = actor("bob", 2);
        let draft = NoteDraft {
            source: "= 共有されたノート\n\n本文 cite:[smith2024]".into(),
            title: "共有されたノート".into(),
            tags: Vec::new(),
        };

        assert_eq!(
            application
                .update_note(
                    editor.clone(),
                    note_id,
                    draft.clone(),
                    Revision::INITIAL,
                    NoteWritePolicy::RejectWarnings,
                )
                .await,
            Err(NoteUseCaseError::Unavailable)
        );

        let created = application
            .create_note(
                editor,
                draft,
                NoteWritePolicy::RejectWarnings,
                NoteCreationSource::Rest,
            )
            .await;
        let Err(NoteUseCaseError::AdvisoriesRejected(diagnostics)) = created else {
            panic!("未登録のcitation keyは新規作成で拒否されます: {created:?}");
        };
        assert_eq!(diagnostics[0].code, "unknown_citation_key");
    }

    #[tokio::test]
    async fn strict_writes_reject_warnings_before_mutation() {
        let repository = Arc::new(MemoryNotes::default());
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor = actor("alice", 1);
        let draft = NoteDraft {
            source: "= Warning\n\nbody".into(),
            title: "Warning".into(),
            tags: Vec::new(),
        };

        let create_error = application
            .create_note(
                actor.clone(),
                draft.clone(),
                NoteWritePolicy::RejectWarnings,
                NoteCreationSource::Rest,
            )
            .await
            .expect_err("warning must reject strict create");
        assert!(matches!(
            create_error,
            NoteUseCaseError::AdvisoriesRejected(_)
        ));
        assert!(repository.notes.lock().expect("notes lock").is_empty());

        let existing = application
            .create_note(
                actor.clone(),
                draft.clone(),
                NoteWritePolicy::AllowAdvisories,
                NoteCreationSource::Rest,
            )
            .await
            .expect("advisory is accepted");
        assert!(matches!(
            application
                .update_note(
                    actor,
                    existing.note_id(),
                    draft,
                    existing.revision(),
                    NoteWritePolicy::RejectWarnings,
                )
                .await,
            Err(NoteUseCaseError::AdvisoriesRejected(_))
        ));
        assert_eq!(repository.update_calls.load(Ordering::Relaxed), 0);
    }

    fn seeded_note(repository: &MemoryNotes, source: &str) -> NoteId {
        let note_id = NoteId::new(
            "0197c9bc-0000-7000-8000-000000000041"
                .parse::<EntityId>()
                .expect("UUIDv7"),
        );
        repository.notes.lock().expect("notes lock").push(
            Note::restore(NoteRestore {
                note_id,
                owner: OneItemLibrary::owner(),
                draft: NoteDraft {
                    title: "対象ノート".into(),
                    source: source.into(),
                    tags: Vec::new(),
                },
                created_at: UnixMillis::new(0),
                updated_at: UnixMillis::new(1),
                revision: Revision::INITIAL,
                deleted_at: None,
                created_via: NoteCreationSource::Mcp,
                review: NoteReviewTracking::pending(),
            })
            .expect("stored note"),
        );
        note_id
    }

    const PATCH: &str = concat!(
        "--- a/note.adoc\n+++ b/note.adoc\n",
        "@@ -3,1 +3,1 @@\n-本文\n+改稿した本文\n",
    );

    /// dry runは適用と検証まで行い、保存を呼ばずに変更量と診断を返す。
    #[tokio::test]
    async fn patch_dry_run_validates_without_saving() {
        let repository = Arc::new(MemoryNotes::default());
        let note_id = seeded_note(&repository, "= 対象ノート\n\n本文\n");
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor = actor("alice", 1);

        let applied = application
            .apply_note_patch(
                actor,
                note_id,
                PATCH,
                Revision::INITIAL,
                NoteWritePolicy::AllowAdvisories,
                true,
            )
            .await
            .expect("dry run");
        assert_eq!(applied.note, None);
        assert_eq!(
            (
                applied.hunks_applied,
                applied.lines_added,
                applied.lines_removed
            ),
            (1, 1, 1)
        );
        // AcceptContentはwarningを1件返す。dry runでも同じ検証で診断を得る。
        assert_eq!(applied.advisories.len(), 1);
        assert_eq!(repository.update_calls.load(Ordering::Relaxed), 0);
    }

    /// warningを含む適用結果は保存前に拒否し、repositoryを呼ばない。
    #[tokio::test]
    async fn patch_rejects_warnings_before_saving() {
        let repository = Arc::new(MemoryNotes::default());
        let note_id = seeded_note(&repository, "= 対象ノート\n\n本文\n");
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor = actor("alice", 1);

        let error = application
            .apply_note_patch(
                actor,
                note_id,
                PATCH,
                Revision::INITIAL,
                NoteWritePolicy::RejectWarnings,
                false,
            )
            .await
            .expect_err("warning rejection");
        assert!(matches!(error, NoteUseCaseError::AdvisoriesRejected(_)));
        assert_eq!(repository.update_calls.load(Ordering::Relaxed), 0);
    }

    /// revisionの不一致は適用前に競合として拒否する。
    #[tokio::test]
    async fn patch_conflicts_on_a_stale_revision() {
        let repository = Arc::new(MemoryNotes::default());
        let note_id = seeded_note(&repository, "= 対象ノート\n\n本文\n");
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor = actor("alice", 1);

        let error = application
            .apply_note_patch(
                actor,
                note_id,
                PATCH,
                Revision::new(2).expect("revision"),
                NoteWritePolicy::AllowAdvisories,
                true,
            )
            .await
            .expect_err("stale revision");
        assert!(matches!(error, NoteUseCaseError::Conflict));
    }

    /// 一致しないhunkは位置つきで拒否し、保存しない。
    #[tokio::test]
    async fn patch_mismatch_is_rejected_with_its_location() {
        let repository = Arc::new(MemoryNotes::default());
        let note_id = seeded_note(&repository, "= 対象ノート\n\n別の本文\n");
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor = actor("alice", 1);

        let error = application
            .apply_note_patch(
                actor,
                note_id,
                PATCH,
                Revision::INITIAL,
                NoteWritePolicy::AllowAdvisories,
                false,
            )
            .await
            .expect_err("hunk mismatch");
        assert_eq!(
            error,
            NoteUseCaseError::PatchRejected(crate::NotePatchError::HunkMismatch {
                hunk: 1,
                source_line: 3
            })
        );
        assert_eq!(repository.update_calls.load(Ordering::Relaxed), 0);
    }

    /// 閲覧だけの利用者には保存経路と同じ分類で拒否する。
    #[tokio::test]
    async fn patch_requires_edit_access() {
        let repository = Arc::new(MemoryNotes::default());
        let note_id = seeded_note(&repository, "= 対象ノート\n\n本文\n");
        *repository.accessible_as.lock().expect("access lock") =
            Some(marginalis_domain::NoteAccess::Read);
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor = actor("bob", 2);

        let error = application
            .apply_note_patch(
                actor,
                note_id,
                PATCH,
                Revision::INITIAL,
                NoteWritePolicy::AllowAdvisories,
                true,
            )
            .await
            .expect_err("read-only actor");
        assert!(matches!(error, NoteUseCaseError::NotFound));
    }

    /// dry runでない適用は、update_noteと同じ条件付き更新を1回だけ呼ぶ。
    #[tokio::test]
    async fn patch_saves_through_the_conditional_update() {
        let repository = Arc::new(MemoryNotes::default());
        let note_id = seeded_note(&repository, "= 対象ノート\n\n本文\n");
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor = actor("alice", 1);

        // repository stubは条件付き更新でUnavailableを返すため、失敗の分類で
        // 保存経路まで到達したことが分かる。
        let error = application
            .apply_note_patch(
                actor,
                note_id,
                PATCH,
                Revision::INITIAL,
                NoteWritePolicy::AllowAdvisories,
                false,
            )
            .await
            .expect_err("stub repository");
        assert!(matches!(error, NoteUseCaseError::Unavailable));
        assert_eq!(repository.update_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn strict_policy_allows_diagnostics_below_warning() {
        let diagnostics = [
            NoteAdvisorySeverity::Information,
            NoteAdvisorySeverity::Hint,
        ]
        .into_iter()
        .map(|severity| NoteAdvisoryDiagnostic {
            code: "test-advisory".into(),
            severity,
            target: NoteValidationTarget::Source,
            span: None,
            position: None,
            message: "test advisory".into(),
        })
        .collect::<Vec<_>>();

        assert_eq!(
            reject_warnings(NoteWritePolicy::RejectWarnings, &diagnostics),
            Ok(())
        );
    }
}
