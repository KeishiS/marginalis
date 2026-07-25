//! ノートの検証、可視性、revision規則を共有するuse case。

use async_trait::async_trait;
use marginalis_application::{Clock, NoteUseCaseError, NoteUseCases, Random};
use marginalis_domain::{Actor, Note, NoteDraft, NoteId, NotePermission};
use marginalis_sqlite::{SqliteDatabase, SqliteStoreError};

use crate::{SystemClock, SystemRandom};

/// transportへノート操作だけを公開するserver側実装。
#[derive(Clone, Debug)]
pub struct ServerNoteUseCases {
    database: SqliteDatabase,
}

impl ServerNoteUseCases {
    pub fn new(database: SqliteDatabase) -> Self {
        Self { database }
    }
}

fn map_note_error(error: SqliteStoreError) -> NoteUseCaseError {
    match error {
        SqliteStoreError::Conflict | SqliteStoreError::LastAdmin => NoteUseCaseError::Conflict,
        SqliteStoreError::CorruptNote | SqliteStoreError::ArchiveFormat => {
            NoteUseCaseError::Validation
        }
        SqliteStoreError::ArchiveTargetNotEmpty
        | SqliteStoreError::ArchiveMissingAdmin
        | SqliteStoreError::Database(_) => NoteUseCaseError::Unavailable,
    }
}

#[async_trait]
impl NoteUseCases for ServerNoteUseCases {
    async fn list_visible_notes(&self, actor: Actor) -> Result<Vec<Note>, NoteUseCaseError> {
        self.database
            .list_visible_notes(&actor, 0, 1_000)
            .await
            .map_err(map_note_error)
    }

    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.database
            .visible_note(&actor, note_id, NotePermission::Read)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::NotFound)
    }

    async fn create_note(&self, actor: Actor, draft: NoteDraft) -> Result<Note, NoteUseCaseError> {
        let draft = marginalis_asciidoc::validate_note_draft(draft)
            .map_err(|_| NoteUseCaseError::Validation)?;
        let now = SystemClock.now();
        let note = Note {
            note_id: NoteId::new(SystemRandom.uuid_v7()),
            creator_issuer: actor.issuer.clone(),
            creator_subject: actor.subject.clone(),
            title: draft.title,
            body: draft.body,
            tags: draft.tags,
            created_at: now,
            updated_at: now,
            revision: 1,
            deleted_at: None,
        };
        self.database
            .create_note(&note, NotePermission::Admin)
            .await
            .map_err(map_note_error)?;
        Ok(note)
    }

    async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        self.database
            .visible_note(&actor, note_id, NotePermission::Write)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::NotFound)?;
        let draft = marginalis_asciidoc::validate_note_draft(draft)
            .map_err(|_| NoteUseCaseError::Validation)?;
        self.database
            .update_note(note_id, expected_revision, &draft, SystemClock.now())
            .await
            .map_err(map_note_error)
    }

    async fn soft_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        self.database
            .visible_note(&actor, note_id, NotePermission::Admin)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::NotFound)?;
        self.database
            .soft_delete_note(note_id, expected_revision, SystemClock.now())
            .await
            .map_err(map_note_error)?;
        self.database
            .note(note_id, true)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::Unavailable)
    }

    async fn restore_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        self.database
            .visible_deleted_note(&actor, note_id)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::NotFound)?;
        self.database
            .restore_note(note_id, expected_revision, SystemClock.now())
            .await
            .map_err(map_note_error)
    }

    fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        marginalis_asciidoc::export_note(note).map_err(|_| NoteUseCaseError::Unavailable)
    }

    fn render_note_html(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        marginalis_asciidoc::render_note_html(note).map_err(|_| NoteUseCaseError::Validation)
    }
}
