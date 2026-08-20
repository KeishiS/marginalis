//! applicationのノートrepository portに対するSQLite実装。

use async_trait::async_trait;
use marginalis_application::{
    AccessibleNote, NoteAclRepository, NoteAclState, NoteCommandRepository, NoteGraph,
    NoteGraphQuery, NoteLinks, NoteListQuery, NoteQueryRepository, NoteReviewRepository,
    NoteRevisionView, NoteSyncPage, NoteSyncRepository, NoteSyncRepositoryError, NoteViewSnapshot,
    StorageError,
};
use marginalis_domain::{
    Actor, AttachmentId, AttachmentMetadata, DeletedNoteListEntry, Note, NoteAclEntry, NoteDraft,
    NoteId, NoteListEntry, NoteRevisionSummary, Revision, StoredAttachment, UnixMillis,
};

use crate::SqliteDatabase;
use crate::notes::RestoreNoteError;

#[async_trait]
impl NoteQueryRepository for SqliteDatabase {
    async fn list_visible_notes(
        &self,
        actor: &Actor,
        query: &NoteListQuery,
    ) -> Result<Vec<NoteListEntry>, StorageError> {
        SqliteDatabase::list_visible_notes(self, actor, query)
            .await
            .map_err(StorageError::from)
    }

    async fn list_owned_deleted_notes(
        &self,
        actor: &Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, StorageError> {
        SqliteDatabase::list_owned_deleted_notes(self, actor)
            .await
            .map_err(StorageError::from)
    }

    async fn accessible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<AccessibleNote>, StorageError> {
        SqliteDatabase::accessible_note(self, actor, note_id)
            .await
            .map_err(StorageError::from)
    }

    async fn visible_notes_by_id(
        &self,
        actor: &Actor,
        note_ids: &[NoteId],
    ) -> Result<Vec<Note>, StorageError> {
        SqliteDatabase::visible_notes_by_id(self, actor, note_ids)
            .await
            .map_err(StorageError::from)
    }

    async fn note_view_snapshot(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteViewSnapshot>, StorageError> {
        SqliteDatabase::note_view_snapshot(self, actor, note_id)
            .await
            .map_err(StorageError::from)
    }

    async fn list_note_revisions(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Vec<NoteRevisionSummary>>, StorageError> {
        SqliteDatabase::list_note_revisions(self, actor, note_id)
            .await
            .map_err(StorageError::from)
    }

    async fn note_revision(
        &self,
        actor: &Actor,
        note_id: NoteId,
        revision: Revision,
    ) -> Result<Option<NoteRevisionView>, StorageError> {
        SqliteDatabase::note_revision(self, actor, note_id, revision)
            .await
            .map_err(StorageError::from)
    }

    async fn list_note_attachments(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Vec<AttachmentMetadata>>, StorageError> {
        SqliteDatabase::list_note_attachments(self, actor, note_id)
            .await
            .map_err(StorageError::from)
    }

    async fn note_attachment(
        &self,
        actor: &Actor,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Result<Option<StoredAttachment>, StorageError> {
        SqliteDatabase::note_attachment(self, actor, note_id, attachment_id)
            .await
            .map_err(StorageError::from)
    }

    async fn note_graph(
        &self,
        actor: &Actor,
        query: &NoteGraphQuery,
    ) -> Result<NoteGraph, StorageError> {
        SqliteDatabase::note_graph(self, actor, query)
            .await
            .map_err(StorageError::from)
    }
}

#[async_trait]
impl NoteSyncRepository for SqliteDatabase {
    async fn sync_notes(
        &self,
        actor: &Actor,
        cursor: Option<&str>,
        limit: usize,
        next_cursor: &str,
        now: UnixMillis,
    ) -> Result<NoteSyncPage, NoteSyncRepositoryError> {
        self.sync_notes_page(actor, cursor, limit, next_cursor, now)
            .await
    }
}

#[async_trait]
impl NoteCommandRepository for SqliteDatabase {
    async fn create_note(&self, note: &Note, links: NoteLinks<'_>) -> Result<(), StorageError> {
        SqliteDatabase::create_note(self, note, links)
            .await
            .map_err(StorageError::from)
    }

    async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        draft: &NoteDraft,
        links: NoteLinks<'_>,
        now: UnixMillis,
    ) -> Result<Note, StorageError> {
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
        .map_err(StorageError::from)
    }

    async fn restore_visible_note_revision(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        draft: &NoteDraft,
        links: NoteLinks<'_>,
        now: UnixMillis,
    ) -> Result<Note, StorageError> {
        SqliteDatabase::restore_visible_note_revision(
            self,
            actor,
            note_id,
            expected_revision,
            draft,
            links,
            now,
        )
        .await
        .map_err(StorageError::from)
    }

    async fn soft_delete_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        now: UnixMillis,
    ) -> Result<Note, StorageError> {
        SqliteDatabase::soft_delete_visible_note(self, actor, note_id, expected_revision, now)
            .await
            .map_err(StorageError::from)
    }

    async fn restore_owned_deleted_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        now: UnixMillis,
    ) -> Result<Note, StorageError> {
        SqliteDatabase::restore_owned_deleted_note(self, actor, note_id, expected_revision, now)
            .await
            .map_err(map_restore_error)
    }

    async fn create_note_attachment(
        &self,
        actor: &Actor,
        attachment: &StoredAttachment,
    ) -> Result<(), StorageError> {
        SqliteDatabase::create_note_attachment(self, actor, attachment)
            .await
            .map_err(StorageError::from)
    }

    async fn delete_unused_note_attachment(
        &self,
        actor: &Actor,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Result<(), StorageError> {
        SqliteDatabase::delete_unused_note_attachment(self, actor, note_id, attachment_id)
            .await
            .map_err(StorageError::from)
    }
}

#[async_trait]
impl NoteAclRepository for SqliteDatabase {
    async fn read_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, StorageError> {
        SqliteDatabase::read_note_acl(self, actor, note_id)
            .await
            .map_err(StorageError::from)
    }

    async fn replace_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
        entries: &[NoteAclEntry],
        expected_revision: Revision,
        now: UnixMillis,
    ) -> Result<Note, StorageError> {
        SqliteDatabase::replace_note_acl(self, actor, note_id, entries, expected_revision, now)
            .await
            .map_err(StorageError::from)
    }
}

#[async_trait]
impl NoteReviewRepository for SqliteDatabase {
    async fn read_owned_note_review(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Note, StorageError> {
        SqliteDatabase::read_owned_note_review(self, actor, note_id)
            .await
            .map_err(StorageError::from)
    }

    async fn mark_owned_note_reviewed(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        reviewed_at: UnixMillis,
    ) -> Result<Note, StorageError> {
        SqliteDatabase::mark_owned_note_reviewed(
            self,
            actor,
            note_id,
            expected_revision,
            reviewed_at,
        )
        .await
        .map_err(StorageError::from)
    }
}

fn map_restore_error(error: RestoreNoteError) -> StorageError {
    match error {
        RestoreNoteError::RetentionExpired => StorageError::RetentionExpired,
        RestoreNoteError::Store(error) => error.into(),
    }
}
