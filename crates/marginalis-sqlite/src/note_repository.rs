//! applicationのノートrepository portに対するSQLite実装。

use async_trait::async_trait;
use marginalis_application::{
    AccessibleNote, NoteAclRepository, NoteAclState, NoteCommandRepository, NoteGraph,
    NoteGraphQuery, NoteLinks, NoteQueryRepository, NoteRepositoryError, NoteViewSnapshot,
};
use marginalis_domain::{
    Actor, DeletedNoteListEntry, Note, NoteAclEntry, NoteDraft, NoteId, NoteListEntry, Revision,
    UnixMillis,
};

use crate::notes::RestoreNoteError;
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

    async fn list_owned_deleted_notes(
        &self,
        actor: &Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, NoteRepositoryError> {
        SqliteDatabase::list_owned_deleted_notes(self, actor)
            .await
            .map_err(map_error)
    }

    async fn accessible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<AccessibleNote>, NoteRepositoryError> {
        SqliteDatabase::accessible_note(self, actor, note_id)
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

    async fn note_graph(
        &self,
        actor: &Actor,
        query: &NoteGraphQuery,
    ) -> Result<NoteGraph, NoteRepositoryError> {
        SqliteDatabase::note_graph(self, actor, query)
            .await
            .map_err(map_error)
    }
}

#[async_trait]
impl NoteCommandRepository for SqliteDatabase {
    async fn create_note(
        &self,
        note: &Note,
        links: NoteLinks<'_>,
    ) -> Result<(), NoteRepositoryError> {
        SqliteDatabase::create_note(self, note, links)
            .await
            .map_err(map_error)
    }

    async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        draft: &NoteDraft,
        links: NoteLinks<'_>,
        now: UnixMillis,
    ) -> Result<Note, NoteRepositoryError> {
        SqliteDatabase::update_visible_note(
            self,
            actor,
            note_id,
            expected_revision,
            draft,
            links,
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

    async fn restore_owned_deleted_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        now: UnixMillis,
    ) -> Result<Note, NoteRepositoryError> {
        SqliteDatabase::restore_owned_deleted_note(self, actor, note_id, expected_revision, now)
            .await
            .map_err(map_restore_error)
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

fn map_restore_error(error: RestoreNoteError) -> NoteRepositoryError {
    match error {
        RestoreNoteError::RetentionExpired => NoteRepositoryError::RetentionExpired,
        RestoreNoteError::Store(error) => map_error(error),
    }
}
