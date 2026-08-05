//! 所有者によるノートの人手確認。

use async_trait::async_trait;
use marginalis_domain::{Actor, Note, NoteId, Revision};

use crate::{NoteReviewDetails, NoteReviews, NoteUseCaseError};

use super::{NoteApplication, map_repository_error};

#[async_trait]
impl NoteReviews for NoteApplication {
    async fn read_note_review(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteReviewDetails, NoteUseCaseError> {
        self.reviews
            .read_owned_note_review(&actor, note_id)
            .await
            .map(review_details)
            .map_err(map_repository_error)
    }

    async fn mark_note_reviewed(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<NoteReviewDetails, NoteUseCaseError> {
        self.reviews
            .mark_owned_note_reviewed(&actor, note_id, expected_revision, self.clock.now())
            .await
            .map(review_details)
            .map_err(map_repository_error)
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
        reviewer: last_review.map(|review| review.reviewer().clone()),
    }
}
