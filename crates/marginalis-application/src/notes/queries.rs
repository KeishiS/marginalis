//! ノート一覧と単一ノートの問い合わせ。

use marginalis_domain::{Actor, DeletedNoteListEntry, Note, NoteId, NoteListEntry};

use crate::{NoteListQuery, NoteUseCaseError};

use super::NoteApplication;

impl NoteApplication {
    pub async fn list_visible_notes(
        &self,
        actor: Actor,
        query: NoteListQuery,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        self.queries
            .list_visible_notes(&actor, &query)
            .await
            .map_err(NoteUseCaseError::from)
    }

    pub async fn list_owned_deleted_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, NoteUseCaseError> {
        self.queries
            .list_owned_deleted_notes(&actor)
            .await
            .map_err(NoteUseCaseError::from)
    }

    pub async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.read_visible_note(&actor, note_id).await
    }
}
