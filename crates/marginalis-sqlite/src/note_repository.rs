//! applicationのノートrepository portに対するSQLite実装。

use async_trait::async_trait;
use marginalis_application::{NoteRepository, NoteRepositoryError};
use marginalis_domain::{
    Actor, Note, NoteAclEntry, NoteCapabilities, NoteDraft, NoteId, NoteSummary, UnixMillis,
};

use crate::{SqliteDatabase, SqliteStoreError};

#[async_trait]
impl NoteRepository for SqliteDatabase {
    async fn list_visible_notes(&self, actor: &Actor) -> Result<Vec<Note>, NoteRepositoryError> {
        SqliteDatabase::list_visible_notes(self, actor)
            .await
            .map_err(map_error)
    }

    async fn visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Note>, NoteRepositoryError> {
        SqliteDatabase::visible_note(self, actor, note_id)
            .await
            .map_err(map_error)
    }

    async fn create_note(
        &self,
        note: &Note,
        reference_targets: &[NoteId],
    ) -> Result<(), NoteRepositoryError> {
        SqliteDatabase::create_note(self, note, reference_targets)
            .await
            .map_err(map_error)
    }

    async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: i64,
        draft: &NoteDraft,
        reference_targets: &[NoteId],
        now: UnixMillis,
    ) -> Result<Note, NoteRepositoryError> {
        SqliteDatabase::update_visible_note(
            self,
            actor,
            note_id,
            expected_revision,
            draft,
            reference_targets,
            now,
        )
        .await
        .map_err(map_error)
    }

    async fn soft_delete_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: i64,
        now: UnixMillis,
    ) -> Result<Note, NoteRepositoryError> {
        SqliteDatabase::soft_delete_visible_note(self, actor, note_id, expected_revision, now)
            .await
            .map_err(map_error)
    }

    async fn restore_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: i64,
        now: UnixMillis,
    ) -> Result<Note, NoteRepositoryError> {
        SqliteDatabase::restore_visible_note(self, actor, note_id, expected_revision, now)
            .await
            .map_err(map_error)
    }

    async fn directly_related_notes(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<(Vec<NoteSummary>, Vec<NoteSummary>), NoteRepositoryError> {
        SqliteDatabase::directly_related_notes(self, actor, note_id)
            .await
            .map_err(map_error)
    }

    async fn note_capabilities(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteCapabilities>, NoteRepositoryError> {
        SqliteDatabase::note_capabilities(self, actor, note_id)
            .await
            .map_err(map_error)
    }

    async fn read_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Vec<NoteAclEntry>, NoteRepositoryError> {
        SqliteDatabase::read_note_acl(self, actor, note_id)
            .await
            .map_err(map_error)
    }

    async fn replace_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
        entries: &[NoteAclEntry],
        expected_revision: i64,
        now: UnixMillis,
    ) -> Result<Note, NoteRepositoryError> {
        SqliteDatabase::replace_note_acl(self, actor, note_id, entries, expected_revision, now)
            .await
            .map_err(map_error)
    }
}

fn map_error(error: SqliteStoreError) -> NoteRepositoryError {
    match error {
        SqliteStoreError::NotFound => NoteRepositoryError::NotFound,
        SqliteStoreError::Conflict => NoteRepositoryError::Conflict,
        SqliteStoreError::CorruptData => NoteRepositoryError::CorruptData,
        SqliteStoreError::ArchiveTargetNotEmpty | SqliteStoreError::Database(_) => {
            NoteRepositoryError::Unavailable
        }
    }
}
