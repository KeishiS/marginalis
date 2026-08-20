//! 所有者によるノートの人手確認。

use marginalis_domain::{Actor, Note, NoteId, Revision};

use crate::{NoteReviewDetails, NoteUseCaseError};

use super::NoteApplication;

impl NoteApplication {
    pub async fn read_note_review(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteReviewDetails, NoteUseCaseError> {
        self.reviews
            .read_owned_note_review(&actor, note_id)
            .await
            .map(review_details)
            .map_err(NoteUseCaseError::from)
    }

    pub async fn mark_note_reviewed(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<NoteReviewDetails, NoteUseCaseError> {
        self.reviews
            .mark_owned_note_reviewed(&actor, note_id, expected_revision, self.clock.now())
            .await
            .map(review_details)
            .map_err(NoteUseCaseError::from)
    }
}

fn review_details(note: Note) -> NoteReviewDetails {
    let last_review = note.last_review();
    NoteReviewDetails {
        note_id: note.note_id(),
        current_revision: note.revision(),
        status: note.review_status(),
        reviewed_revision: last_review.map(|review| review.revision()),
        reviewed_at: last_review.map(|review| review.reviewed_at()),
        reviewer: last_review.map(|review| review.reviewer().primary_identity().clone()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use marginalis_domain::{NoteCreationSource, NoteDraft, NoteReviewStatus, Revision};

    use crate::NoteWritePolicy;

    use crate::notes::test_support::{
        AcceptContent, EmptyLibrary, MemoryNotes, NoMathMacros, actor, note_application,
    };

    #[tokio::test]
    async fn owner_review_uses_the_application_clock_and_returns_the_public_projection() {
        let repository = Arc::new(MemoryNotes::default());
        let application = note_application(
            &repository,
            Arc::new(AcceptContent::default()),
            Arc::new(EmptyLibrary),
            Arc::new(NoMathMacros),
        );
        let actor = actor("alice", 1);
        let note = application
            .create_note(
                actor.clone(),
                NoteDraft {
                    source: "= 確認対象\n\n本文".into(),
                    title: "確認対象".into(),
                    tags: Vec::new(),
                },
                NoteWritePolicy::AllowAdvisories,
                NoteCreationSource::Web,
            )
            .await
            .expect("create note");

        let pending = application
            .read_note_review(actor.clone(), note.note_id())
            .await
            .expect("read pending review");
        assert_eq!(pending.status, NoteReviewStatus::Pending);
        assert_eq!(pending.reviewer, None);

        let reviewed = application
            .mark_note_reviewed(actor.clone(), note.note_id(), Revision::INITIAL)
            .await
            .expect("mark reviewed");
        assert_eq!(reviewed.current_revision.get(), 2);
        assert_eq!(reviewed.reviewed_revision, Some(reviewed.current_revision));
        assert_eq!(reviewed.status, NoteReviewStatus::Reviewed);
        assert_eq!(
            reviewed.reviewed_at.map(|time| time.get()),
            Some(1_700_000_000_000)
        );
        assert_eq!(
            reviewed.reviewer.as_ref(),
            Some(actor.principal().primary_identity())
        );

        assert_eq!(
            application
                .mark_note_reviewed(actor, note.note_id(), Revision::INITIAL)
                .await,
            Err(crate::NoteUseCaseError::Conflict)
        );
    }
}
