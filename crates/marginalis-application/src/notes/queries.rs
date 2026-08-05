//! ノート一覧と単一ノートの問い合わせ。

use async_trait::async_trait;
use marginalis_domain::{Actor, DeletedNoteListEntry, Note, NoteId, NoteListEntry};

use crate::{NoteQueries, NoteUseCaseError};

use super::{NoteApplication, map_repository_error};

#[async_trait]
impl NoteQueries for NoteApplication {
    async fn list_visible_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        self.queries
            .list_visible_notes(&actor)
            .await
            .map_err(map_repository_error)
    }

    async fn list_owned_deleted_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, NoteUseCaseError> {
        self.queries
            .list_owned_deleted_notes(&actor)
            .await
            .map_err(map_repository_error)
    }

    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.read_visible_note(&actor, note_id).await
    }
}
