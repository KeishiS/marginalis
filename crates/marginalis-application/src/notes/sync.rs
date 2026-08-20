use marginalis_domain::{Actor, Note, NoteId, UnixMillis};

use super::{NoteApplication, NoteSyncRepositoryError};
use crate::NoteUseCaseError;

pub const NOTE_SYNC_DEFAULT_PAGE_SIZE: usize = 50;
pub const NOTE_SYNC_MAX_PAGE_SIZE: usize = 100;
pub const NOTE_SYNC_CURSOR_RETENTION_MS: i64 = 35 * 24 * 60 * 60 * 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteSyncPhase {
    Snapshot,
    Changes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteSyncRemovalReason {
    Deleted,
    AccessRevoked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteSyncEntry {
    Upsert(Box<Note>),
    Remove {
        note_id: NoteId,
        reason: NoteSyncRemovalReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteSyncPage {
    pub phase: NoteSyncPhase,
    pub entries: Vec<NoteSyncEntry>,
    pub next_cursor: String,
    pub has_more: bool,
    pub cursor_expires_at: UnixMillis,
}

impl NoteApplication {
    pub async fn sync_notes(
        &self,
        actor: Actor,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<NoteSyncPage, NoteUseCaseError> {
        let limit = limit.unwrap_or(NOTE_SYNC_DEFAULT_PAGE_SIZE);
        if !(1..=NOTE_SYNC_MAX_PAGE_SIZE).contains(&limit) {
            return Err(NoteUseCaseError::InvalidSyncLimit);
        }
        let next_cursor = self.random.opaque_token();
        self.notes
            .sync_notes(
                &actor,
                cursor.as_deref(),
                limit,
                &next_cursor,
                self.clock.now(),
            )
            .await
            .map_err(|error| match error {
                NoteSyncRepositoryError::InvalidCursor => NoteUseCaseError::InvalidSyncCursor,
                NoteSyncRepositoryError::CursorExpired => NoteUseCaseError::SyncCursorExpired,
                NoteSyncRepositoryError::Storage(error) => error.into(),
            })
    }
}
