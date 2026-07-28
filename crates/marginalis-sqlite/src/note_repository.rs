//! applicationのノートrepository portに対するSQLite実装。

use async_trait::async_trait;
use marginalis_application::{
    NoteAclRepository, NoteAclState, NoteCommandRepository, NoteQueryRepository,
    NoteRepositoryError, NoteViewSnapshot,
};
use marginalis_domain::{
    Actor, Note, NoteAclEntry, NoteDraft, NoteId, NoteListEntry, Revision, UnixMillis,
};

use crate::{SqliteDatabase, SqliteStoreError};

#[async_trait]
impl NoteQueryRepository for SqliteDatabase {
    async fn list_visible_notes(
        &self,
        actor: &Actor,
    ) -> Result<Vec<NoteListEntry>, NoteRepositoryError> {
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

    async fn visible_notes_by_id(
        &self,
        actor: &Actor,
        note_ids: &[NoteId],
    ) -> Result<Vec<Note>, NoteRepositoryError> {
        SqliteDatabase::visible_notes_by_id(self, actor, note_ids)
            .await
            .map_err(map_error)
    }

    async fn note_view_snapshot(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteViewSnapshot>, NoteRepositoryError> {
        SqliteDatabase::note_view_snapshot(self, actor, note_id)
            .await
            .map_err(map_error)
    }
}

#[async_trait]
impl NoteCommandRepository for SqliteDatabase {
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
        expected_revision: Revision,
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
        expected_revision: Revision,
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
        expected_revision: Revision,
        now: UnixMillis,
    ) -> Result<Note, NoteRepositoryError> {
        SqliteDatabase::restore_visible_note(self, actor, note_id, expected_revision, now)
            .await
            .map_err(map_error)
    }
}

#[async_trait]
impl NoteAclRepository for SqliteDatabase {
    async fn read_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, NoteRepositoryError> {
        SqliteDatabase::read_note_acl(self, actor, note_id)
            .await
            .map_err(map_error)
    }

    async fn replace_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
        entries: &[NoteAclEntry],
        expected_revision: Revision,
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
