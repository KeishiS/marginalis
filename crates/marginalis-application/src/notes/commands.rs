//! ノートの作成、更新、論理削除、復元。

use marginalis_domain::{Actor, Note, NoteCreationSource, NoteDraft, NoteId, Revision};

use crate::{NoteUseCaseError, NoteWritePolicy, ValidatedNoteDraft};

use super::{NoteApplication, NoteLinks, cited_keys, reference_targets};

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
            citation_style,
        } = validated;
        // 新規作成では操作している利用者がそのまま作成者になるため、閲覧時の解決先と一致する。
        diagnostics.extend(
            self.citation_resolutions(actor.identity(), &citation_queries, citation_style)
                .await?
                .diagnostics,
        );
        reject_warnings(policy, diagnostics)?;
        let now = self.clock.now();
        let note = Note::create(
            NoteId::new(self.random.uuid_v7()),
            actor.identity(),
            draft,
            now,
            created_via,
        );
        let reference_targets = reference_targets(&reference_queries);
        let cited_keys = cited_keys(&citation_queries);
        self.commands
            .create_note(
                &note,
                NoteLinks {
                    reference_targets: &reference_targets,
                    cited_keys: &cited_keys,
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
            citation_style,
        } = validated;
        if !citation_queries.is_empty() {
            // 引用は閲覧時に作成者のライブラリーで解決する。共有されたノートを別の利用者が
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
        reject_warnings(policy, diagnostics)?;
        let reference_targets = reference_targets(&reference_queries);
        let cited_keys = cited_keys(&citation_queries);
        self.commands
            .update_visible_note(
                &actor,
                note_id,
                expected_revision,
                &draft,
                NoteLinks {
                    reference_targets: &reference_targets,
                    cited_keys: &cited_keys,
                },
                self.clock.now(),
            )
            .await
            .map_err(NoteUseCaseError::from)
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
    diagnostics: Vec<crate::NoteAdvisoryDiagnostic>,
) -> Result<(), NoteUseCaseError> {
    if policy == NoteWritePolicy::RejectWarnings
        && diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::NoteAdvisorySeverity::Warning)
    {
        return Err(NoteUseCaseError::AdvisoriesRejected(diagnostics));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::Ordering};

    use marginalis_domain::{
        Actor, EntityId, Note, NoteCreationSource, NoteDraft, NoteId, NoteRestore,
        NoteReviewTracking, Revision, UnixMillis,
    };

    use crate::{NoteAdvisoryDiagnostic, NoteAdvisorySeverity, NoteValidationTarget};

    use super::*;
    use crate::notes::test_support::{
        AcceptContent, CitingContent, EmptyLibrary, MemoryNotes, NoMathMacros, OneItemLibrary,
        OwnerMathMacros, note_application,
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
        let actor =
            Actor::try_new("https://id.example.test".into(), "alice".into()).expect("valid actor");

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

        assert_eq!(created.creator_subject(), "alice");
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
        let editor =
            Actor::try_new("https://id.example.test".into(), "bob".into()).expect("valid actor");
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
        let actor =
            Actor::try_new("https://id.example.test".into(), "alice".into()).expect("valid actor");
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
        .collect();

        assert_eq!(
            reject_warnings(NoteWritePolicy::RejectWarnings, diagnostics),
            Ok(())
        );
    }
}
