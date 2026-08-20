//! ノート履歴の参照、行単位diff、過去の本文を新しいrevisionとして保存する処理。

use marginalis_domain::{Actor, Note, NoteDraft, NoteId, NoteRevisionSummary, Revision};
use similar::TextDiff;

use crate::{NoteUseCaseError, NoteWritePolicy, ValidatedNoteDraft};

use super::{
    NoteApplication, NoteLinks, NoteRevisionView, attachment_ids, cited_keys,
    commands::reject_warnings, reference_targets,
};

/// 保存せず要求時に生成する、二つのrevision間のUnified Diff。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteRevisionDiff {
    pub from_revision: Revision,
    pub to_revision: Revision,
    pub unified_diff: String,
}

impl NoteApplication {
    pub async fn list_note_revisions(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<Vec<NoteRevisionSummary>, NoteUseCaseError> {
        self.queries
            .list_note_revisions(&actor, note_id)
            .await
            .map_err(NoteUseCaseError::from)?
            .ok_or(NoteUseCaseError::NotFound)
    }

    pub async fn read_note_revision(
        &self,
        actor: Actor,
        note_id: NoteId,
        revision: Revision,
    ) -> Result<NoteRevisionView, NoteUseCaseError> {
        self.queries
            .note_revision(&actor, note_id, revision)
            .await
            .map_err(NoteUseCaseError::from)?
            .ok_or(NoteUseCaseError::NotFound)
    }

    pub async fn compare_note_revisions(
        &self,
        actor: Actor,
        note_id: NoteId,
        from_revision: Revision,
        to_revision: Revision,
    ) -> Result<NoteRevisionDiff, NoteUseCaseError> {
        let from = self
            .read_note_revision(actor.clone(), note_id, from_revision)
            .await?;
        let to = self.read_note_revision(actor, note_id, to_revision).await?;
        let from_label = format!("revision-{}", from_revision.get());
        let to_label = format!("revision-{}", to_revision.get());
        let unified_diff =
            TextDiff::from_lines(from.revision.note().source(), to.revision.note().source())
                .unified_diff()
                .context_radius(3)
                .header(&from_label, &to_label)
                .to_string();
        Ok(NoteRevisionDiff {
            from_revision,
            to_revision,
            unified_diff,
        })
    }

    pub async fn restore_note_revision(
        &self,
        actor: Actor,
        note_id: NoteId,
        revision: Revision,
        expected_revision: Revision,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        let historical = self
            .read_note_revision(actor.clone(), note_id, revision)
            .await?;
        let validated = self
            .content
            .validate_draft(NoteDraft {
                source: historical.revision.note().source().to_owned(),
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
            .restore_visible_note_revision(
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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use marginalis_domain::{
        NoteCreationSource, NoteRestore, NoteReviewTracking, NoteRevisionKind,
        NoteRevisionSnapshot, UnixMillis,
    };

    use super::*;
    use crate::notes::test_support::{
        AcceptContent, EmptyLibrary, MemoryNotes, NoMathMacros, actor, note_application,
    };

    #[tokio::test]
    async fn compares_lines_and_restores_history_as_a_new_revision() {
        let repository = Arc::new(MemoryNotes::default());
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let alice = actor("alice", 1);
        let first = application
            .create_note(
                alice.clone(),
                NoteDraft {
                    title: "履歴".into(),
                    source: "= 履歴\n\n最初\n共通\n".into(),
                    tags: Vec::new(),
                },
                NoteWritePolicy::AllowAdvisories,
                NoteCreationSource::Web,
            )
            .await
            .expect("create note");
        let second = Note::restore(NoteRestore {
            note_id: first.note_id(),
            owner: first.owner().clone(),
            draft: NoteDraft {
                title: "履歴".into(),
                source: "= 履歴\n\n二番目\n共通\n".into(),
                tags: Vec::new(),
            },
            created_at: first.created_at(),
            updated_at: UnixMillis::new(first.updated_at().get() + 1),
            revision: Revision::new(2).expect("revision"),
            deleted_at: None,
            created_via: first.created_via(),
            review: NoteReviewTracking::pending(),
        })
        .expect("second revision");
        repository
            .histories
            .lock()
            .expect("history lock")
            .push(NoteRevisionSnapshot::new(
                second.clone(),
                alice.principal().clone(),
                NoteRevisionKind::ContentUpdated,
            ));
        repository.notes.lock().expect("notes lock")[0] = second;

        let diff = application
            .compare_note_revisions(
                alice.clone(),
                first.note_id(),
                Revision::INITIAL,
                Revision::new(2).expect("revision"),
            )
            .await
            .expect("compare history");
        assert!(diff.unified_diff.contains("--- revision-1"));
        assert!(diff.unified_diff.contains("+++ revision-2"));
        assert!(diff.unified_diff.contains("-最初"));
        assert!(diff.unified_diff.contains("+二番目"));

        let restored = application
            .restore_note_revision(
                alice,
                first.note_id(),
                Revision::INITIAL,
                Revision::new(2).expect("revision"),
                NoteWritePolicy::AllowAdvisories,
            )
            .await
            .expect("restore history");
        assert_eq!(restored.revision().get(), 3);
        assert_eq!(restored.source(), first.source());
        assert_eq!(
            repository
                .histories
                .lock()
                .expect("history lock")
                .last()
                .expect("restored history")
                .kind(),
            NoteRevisionKind::HistoryRestored
        );
    }
}
