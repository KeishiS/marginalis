//! 検索用投影へ渡す初期一覧、変更、actorに結び付けた不透明cursor。

use std::str::FromStr;

use marginalis_application::{
    NOTE_SYNC_CURSOR_RETENTION_MS, NoteSyncEntry, NoteSyncPage, NoteSyncPhase,
    NoteSyncRemovalReason, NoteSyncRepositoryError,
};
use marginalis_domain::{Actor, EntityId, NoteId, UnixMillis};
use sqlx::Row;

use crate::{
    SqliteDatabase, SqliteStoreError, database_error, notes::note_from_row, token::hash_token,
};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

impl SqliteDatabase {
    pub(crate) async fn sync_notes_page(
        &self,
        actor: &Actor,
        cursor: Option<&str>,
        limit: usize,
        next_cursor: &str,
        now: UnixMillis,
    ) -> Result<NoteSyncPage, NoteSyncRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(database_error_as_storage)?;
        let state = if let Some(cursor) = cursor {
            let row = sqlx::query(
                "SELECT principal_id, phase, after_note_id, after_sequence,
                        high_watermark, expires_at_ms
                 FROM note_sync_cursors WHERE cursor_hash = ?",
            )
            .bind(hash_token(cursor))
            .fetch_optional(&mut *transaction)
            .await
            .map_err(database_error_as_storage)?;
            let Some(row) = row else {
                return Err(NoteSyncRepositoryError::InvalidCursor);
            };
            let principal_id: i64 = row.try_get("principal_id").map_err(corrupt)?;
            if principal_id != actor.principal_id().get() {
                return Err(NoteSyncRepositoryError::InvalidCursor);
            }
            let expires_at: i64 = row.try_get("expires_at_ms").map_err(corrupt)?;
            if expires_at <= now.get() {
                return Err(NoteSyncRepositoryError::CursorExpired);
            }
            CursorState {
                phase: match row.try_get::<String, _>("phase").map_err(corrupt)?.as_str() {
                    "snapshot" => NoteSyncPhase::Snapshot,
                    "changes" => NoteSyncPhase::Changes,
                    _ => {
                        return Err(NoteSyncRepositoryError::Storage(
                            marginalis_application::StorageError::CorruptData,
                        ));
                    }
                },
                after_note_id: row.try_get("after_note_id").map_err(corrupt)?,
                after_sequence: row.try_get("after_sequence").map_err(corrupt)?,
                high_watermark: row.try_get("high_watermark").map_err(corrupt)?,
            }
        } else {
            CursorState {
                phase: NoteSyncPhase::Snapshot,
                after_note_id: None,
                after_sequence: 0,
                high_watermark: sqlx::query_scalar(
                    "SELECT next_sequence FROM note_sync_state WHERE singleton = 1",
                )
                .fetch_one(&mut *transaction)
                .await
                .map_err(database_error_as_storage)?,
            }
        };

        let expires_at = UnixMillis::new(now.get().saturating_add(NOTE_SYNC_CURSOR_RETENTION_MS));
        let (entries, has_more, next_state) = match state.phase {
            NoteSyncPhase::Snapshot => {
                let rows = sqlx::query(
                    "SELECT notes.* FROM note_details notes
                     JOIN note_access access ON access.note_id = notes.note_id
                     WHERE notes.deleted_at_ms IS NULL
                       AND access.principal_id = ?
                       AND (?2 IS NULL OR notes.note_id > ?2)
                     ORDER BY notes.note_id LIMIT ?3",
                )
                .bind(actor.principal_id().get())
                .bind(state.after_note_id.as_deref())
                .bind((limit + 1) as i64)
                .fetch_all(&mut *transaction)
                .await
                .map_err(database_error_as_storage)?;
                let more_by_count = rows.len() > limit;
                let mut entries = Vec::new();
                let mut bytes = 0;
                let mut truncated_by_bytes = false;
                for row in rows.into_iter().take(limit) {
                    let note = note_from_row(row).map_err(storage)?;
                    let estimate = 6
                        * (note.source().len()
                            + note.title().len()
                            + note.tags().iter().map(String::len).sum::<usize>())
                        + 1_024;
                    if !entries.is_empty() && bytes + estimate > MAX_RESPONSE_BYTES {
                        truncated_by_bytes = true;
                        break;
                    }
                    bytes += estimate;
                    entries.push(NoteSyncEntry::Upsert(Box::new(note)));
                }
                let after_note_id = entries.last().and_then(|entry| match entry {
                    NoteSyncEntry::Upsert(note) => Some(note.note_id().to_string()),
                    NoteSyncEntry::Remove { .. } => None,
                });
                let has_more = more_by_count || truncated_by_bytes;
                let next = if has_more {
                    CursorState {
                        after_note_id,
                        ..state.clone()
                    }
                } else {
                    CursorState {
                        phase: NoteSyncPhase::Changes,
                        after_note_id: None,
                        after_sequence: state.high_watermark,
                        high_watermark: state.high_watermark,
                    }
                };
                (entries, has_more, next)
            }
            NoteSyncPhase::Changes => {
                let rows = sqlx::query(
                    "SELECT change_sequence, note_id, kind, reason
                     FROM note_sync_changes
                     WHERE principal_id = ? AND change_sequence > ?
                     ORDER BY change_sequence LIMIT ?",
                )
                .bind(actor.principal_id().get())
                .bind(state.after_sequence)
                .bind((limit + 1) as i64)
                .fetch_all(&mut *transaction)
                .await
                .map_err(database_error_as_storage)?;
                let more_by_count = rows.len() > limit;
                let mut entries = Vec::new();
                let mut after_sequence = state.after_sequence;
                let mut bytes = 0;
                let mut truncated_by_bytes = false;
                for row in rows.into_iter().take(limit) {
                    let sequence = row.try_get("change_sequence").map_err(corrupt)?;
                    let note_id = parse_note_id(row.try_get("note_id").map_err(corrupt)?)?;
                    let kind: String = row.try_get("kind").map_err(corrupt)?;
                    let entry = if kind == "upsert" {
                        let note_row = sqlx::query(
                            "SELECT notes.* FROM note_details notes JOIN note_access access
                             ON access.note_id = notes.note_id
                             WHERE notes.note_id = ? AND notes.deleted_at_ms IS NULL
                               AND access.principal_id = ?",
                        )
                        .bind(note_id.to_string())
                        .bind(actor.principal_id().get())
                        .fetch_optional(&mut *transaction)
                        .await
                        .map_err(database_error_as_storage)?;
                        if let Some(note_row) = note_row {
                            NoteSyncEntry::Upsert(Box::new(
                                note_from_row(note_row).map_err(storage)?,
                            ))
                        } else {
                            NoteSyncEntry::Remove {
                                note_id,
                                reason: NoteSyncRemovalReason::AccessRevoked,
                            }
                        }
                    } else {
                        let reason: String = row.try_get("reason").map_err(corrupt)?;
                        NoteSyncEntry::Remove {
                            note_id,
                            reason: match reason.as_str() {
                                "deleted" => NoteSyncRemovalReason::Deleted,
                                "access_revoked" => NoteSyncRemovalReason::AccessRevoked,
                                _ => {
                                    return Err(NoteSyncRepositoryError::Storage(
                                        marginalis_application::StorageError::CorruptData,
                                    ));
                                }
                            },
                        }
                    };
                    let estimate = match &entry {
                        NoteSyncEntry::Upsert(note) => {
                            6 * (note.source().len()
                                + note.title().len()
                                + note.tags().iter().map(String::len).sum::<usize>())
                                + 1_024
                        }
                        NoteSyncEntry::Remove { .. } => 256,
                    };
                    if !entries.is_empty() && bytes + estimate > MAX_RESPONSE_BYTES {
                        truncated_by_bytes = true;
                        break;
                    }
                    bytes += estimate;
                    after_sequence = sequence;
                    entries.push(entry);
                }
                let has_more = more_by_count || truncated_by_bytes;
                let next = CursorState {
                    after_sequence,
                    ..state.clone()
                };
                (entries, has_more, next)
            }
        };

        sqlx::query(
            "INSERT INTO note_sync_cursors (
                cursor_hash, principal_id, phase, after_note_id, after_sequence,
                high_watermark, expires_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(hash_token(next_cursor))
        .bind(actor.principal_id().get())
        .bind(match next_state.phase {
            NoteSyncPhase::Snapshot => "snapshot",
            NoteSyncPhase::Changes => "changes",
        })
        .bind(next_state.after_note_id)
        .bind(next_state.after_sequence)
        .bind(next_state.high_watermark)
        .bind(expires_at.get())
        .execute(&mut *transaction)
        .await
        .map_err(database_error_as_storage)?;
        transaction
            .commit()
            .await
            .map_err(database_error_as_storage)?;
        Ok(NoteSyncPage {
            phase: state.phase,
            entries,
            next_cursor: next_cursor.to_owned(),
            has_more,
            cursor_expires_at: expires_at,
        })
    }
}

#[derive(Clone)]
struct CursorState {
    phase: NoteSyncPhase,
    after_note_id: Option<String>,
    after_sequence: i64,
    high_watermark: i64,
}

fn parse_note_id(value: String) -> Result<NoteId, NoteSyncRepositoryError> {
    EntityId::from_str(&value).map(NoteId::new).map_err(|_| {
        NoteSyncRepositoryError::Storage(marginalis_application::StorageError::CorruptData)
    })
}

fn storage(error: SqliteStoreError) -> NoteSyncRepositoryError {
    NoteSyncRepositoryError::Storage(error.into())
}
fn database_error_as_storage(error: sqlx::Error) -> NoteSyncRepositoryError {
    storage(database_error(error))
}
fn corrupt(_: sqlx::Error) -> NoteSyncRepositoryError {
    NoteSyncRepositoryError::Storage(marginalis_application::StorageError::CorruptData)
}
