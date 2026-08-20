//! ノートの版履歴を現在のACLで認可し、完全なsnapshotとして保存する。

use marginalis_domain::{
    Actor, AttachmentId, Identity, Note, NoteCreationSource, NoteDraft, NoteId, NoteRestore,
    NoteReviewRecord, NoteReviewTracking, NoteRevisionKind, NoteRevisionSnapshot,
    NoteRevisionSummary, PrincipalId, PrincipalRef, Revision, UnixMillis,
};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    SqliteDatabase, SqliteStoreError, database_error,
    notes::{access_from_level, note_id_from_text},
};
use marginalis_application::NoteRevisionView;

impl SqliteDatabase {
    pub async fn list_note_revisions(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Vec<NoteRevisionSummary>>, SqliteStoreError> {
        let rows = sqlx::query(
            "SELECT history.revision, history.changed_at_ms, history.change_kind,
                    history.changed_by_principal_id, identity.issuer, identity.subject
             FROM note_revisions history
             JOIN notes ON notes.note_id = history.note_id
             JOIN principal_identities identity
               ON identity.principal_id = history.changed_by_principal_id
              AND identity.is_primary = 1
             WHERE history.note_id = ?
               AND ((notes.deleted_at_ms IS NULL AND EXISTS (
                        SELECT 1 FROM note_access access
                        WHERE access.note_id = notes.note_id AND access.principal_id = ?
                    )) OR (notes.deleted_at_ms IS NOT NULL
                           AND notes.creator_principal_id = ?))
             ORDER BY history.revision DESC",
        )
        .bind(note_id.to_string())
        .bind(actor.principal_id().get())
        .bind(actor.principal_id().get())
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;
        if rows.is_empty() {
            return Ok(None);
        }
        rows.into_iter()
            .map(|row| {
                let changed_by =
                    principal_from_columns(&row, "changed_by_principal_id", "issuer", "subject")?;
                Ok(NoteRevisionSummary {
                    revision: Revision::new(row.try_get("revision").map_err(database_error)?)
                        .map_err(|_| SqliteStoreError::CorruptData)?,
                    changed_at: UnixMillis::new(
                        row.try_get("changed_at_ms").map_err(database_error)?,
                    ),
                    changed_by,
                    kind: row
                        .try_get::<String, _>("change_kind")
                        .map_err(database_error)?
                        .parse()
                        .map_err(|_| SqliteStoreError::CorruptData)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }

    pub async fn note_revision(
        &self,
        actor: &Actor,
        note_id: NoteId,
        revision: Revision,
    ) -> Result<Option<NoteRevisionView>, SqliteStoreError> {
        let row = sqlx::query(
            "SELECT history.note_id, notes.creator_principal_id,
                    owner_identity.issuer AS creator_issuer,
                    owner_identity.subject AS creator_subject,
                    history.title, history.source, history.tags_json,
                    notes.created_at_ms, history.changed_at_ms AS updated_at_ms,
                    history.revision, history.deleted_at_ms, notes.created_via,
                    history.review_tracking_known, history.reviewed_revision,
                    history.reviewed_at_ms, history.reviewer_principal_id,
                    reviewer_identity.issuer AS reviewer_issuer,
                    reviewer_identity.subject AS reviewer_subject,
                    history.changed_by_principal_id,
                    changed_identity.issuer AS changed_by_issuer,
                    changed_identity.subject AS changed_by_subject,
                    history.change_kind,
                    CASE WHEN notes.deleted_at_ms IS NOT NULL THEN 3
                         ELSE (SELECT access.access_level FROM note_access access
                               WHERE access.note_id = notes.note_id
                                 AND access.principal_id = ?)
                    END AS access_level
             FROM note_revisions history
             JOIN notes ON notes.note_id = history.note_id
             JOIN principal_identities owner_identity
               ON owner_identity.principal_id = notes.creator_principal_id
              AND owner_identity.is_primary = 1
             JOIN principal_identities changed_identity
               ON changed_identity.principal_id = history.changed_by_principal_id
              AND changed_identity.is_primary = 1
             LEFT JOIN principal_identities reviewer_identity
               ON reviewer_identity.principal_id = history.reviewer_principal_id
              AND reviewer_identity.is_primary = 1
             WHERE history.note_id = ? AND history.revision = ?
               AND ((notes.deleted_at_ms IS NULL AND EXISTS (
                        SELECT 1 FROM note_access access
                        WHERE access.note_id = notes.note_id AND access.principal_id = ?
                    )) OR (notes.deleted_at_ms IS NOT NULL
                           AND notes.creator_principal_id = ?))",
        )
        .bind(actor.principal_id().get())
        .bind(note_id.to_string())
        .bind(revision.get())
        .bind(actor.principal_id().get())
        .bind(actor.principal_id().get())
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        row.map(|row| {
            let access = access_from_level(
                row.try_get::<i64, _>("access_level")
                    .map_err(database_error)?,
            )?;
            Ok(NoteRevisionView {
                revision: revision_from_row(row)?,
                access,
            })
        })
        .transpose()
    }
}

/// ノート更新と同じtransactionで、更新後の状態を一件だけ追記する。
pub(crate) async fn insert_note_revision(
    transaction: &mut Transaction<'_, Sqlite>,
    note_id: NoteId,
    changed_by: PrincipalId,
    kind: NoteRevisionKind,
) -> Result<(), SqliteStoreError> {
    let result = sqlx::query(
        "INSERT INTO note_revisions (
            note_id, revision, changed_at_ms, changed_by_principal_id, change_kind,
            title, source, tags_json, deleted_at_ms, review_tracking_known,
            reviewed_revision, reviewed_at_ms, reviewer_principal_id
         )
         SELECT note_id, revision, updated_at_ms, ?, ?, title, source, tags_json,
                deleted_at_ms, review_tracking_known, reviewed_revision, reviewed_at_ms,
                reviewer_principal_id
         FROM notes WHERE note_id = ?",
    )
    .bind(changed_by.get())
    .bind(kind.as_str())
    .bind(note_id.to_string())
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    if result.rows_affected() != 1 {
        return Err(SqliteStoreError::CorruptData);
    }
    // 本文以外の操作では直前版と同じ参照集合になる。本文を変更する経路は、この後に
    // `replace_note_revision_attachments`で現在版だけを置き換える。
    sqlx::query(
        "INSERT INTO note_revision_attachments (note_id, revision, attachment_id)
         SELECT history.note_id, history.revision, previous.attachment_id
         FROM note_revisions history
         JOIN note_revision_attachments previous
           ON previous.note_id = history.note_id
          AND previous.revision = history.revision - 1
         WHERE history.note_id = ?
           AND history.revision = (SELECT revision FROM notes WHERE note_id = ?)",
    )
    .bind(note_id.to_string())
    .bind(note_id.to_string())
    .execute(&mut **transaction)
    .await
    .map_err(database_error)?;
    Ok(())
}

/// 本文解析で確定した添付集合を、現在のrevisionへ原子的に結び付ける。
pub(crate) async fn replace_note_revision_attachments(
    transaction: &mut Transaction<'_, Sqlite>,
    note_id: NoteId,
    attachment_ids: &[AttachmentId],
) -> Result<(), SqliteStoreError> {
    let revision = sqlx::query_scalar::<_, i64>("SELECT revision FROM notes WHERE note_id = ?")
        .bind(note_id.to_string())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(database_error)?
        .ok_or(SqliteStoreError::CorruptData)?;
    sqlx::query("DELETE FROM note_revision_attachments WHERE note_id = ? AND revision = ?")
        .bind(note_id.to_string())
        .bind(revision)
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;
    for attachment_id in attachment_ids {
        let result = sqlx::query(
            "INSERT INTO note_revision_attachments (note_id, revision, attachment_id)
             SELECT ?, ?, attachment_id FROM note_attachments
             WHERE note_id = ? AND attachment_id = ?",
        )
        .bind(note_id.to_string())
        .bind(revision)
        .bind(note_id.to_string())
        .bind(attachment_id.to_string())
        .execute(&mut **transaction)
        .await
        .map_err(database_error)?;
        if result.rows_affected() != 1 {
            return Err(SqliteStoreError::CorruptData);
        }
    }
    Ok(())
}

pub(crate) async fn all_note_revisions(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<Vec<NoteRevisionSnapshot>, SqliteStoreError> {
    let rows = sqlx::query(
        "SELECT history.note_id, notes.creator_principal_id,
                owner_identity.issuer AS creator_issuer,
                owner_identity.subject AS creator_subject,
                history.title, history.source, history.tags_json,
                notes.created_at_ms, history.changed_at_ms AS updated_at_ms,
                history.revision, history.deleted_at_ms, notes.created_via,
                history.review_tracking_known, history.reviewed_revision,
                history.reviewed_at_ms, history.reviewer_principal_id,
                reviewer_identity.issuer AS reviewer_issuer,
                reviewer_identity.subject AS reviewer_subject,
                history.changed_by_principal_id,
                changed_identity.issuer AS changed_by_issuer,
                changed_identity.subject AS changed_by_subject,
                history.change_kind
         FROM note_revisions history
         JOIN notes ON notes.note_id = history.note_id
         JOIN principal_identities owner_identity
           ON owner_identity.principal_id = notes.creator_principal_id
          AND owner_identity.is_primary = 1
         JOIN principal_identities changed_identity
           ON changed_identity.principal_id = history.changed_by_principal_id
          AND changed_identity.is_primary = 1
         LEFT JOIN principal_identities reviewer_identity
           ON reviewer_identity.principal_id = history.reviewer_principal_id
          AND reviewer_identity.is_primary = 1
         ORDER BY history.note_id, history.revision",
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(database_error)?;
    rows.into_iter().map(revision_from_row).collect()
}

fn revision_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<NoteRevisionSnapshot, SqliteStoreError> {
    let note_id = note_id_from_text(row.try_get("note_id").map_err(database_error)?)?;
    let owner = principal_from_columns(
        &row,
        "creator_principal_id",
        "creator_issuer",
        "creator_subject",
    )?;
    let revision = Revision::new(row.try_get("revision").map_err(database_error)?)
        .map_err(|_| SqliteStoreError::CorruptData)?;
    let changed_at = UnixMillis::new(row.try_get("updated_at_ms").map_err(database_error)?);
    let review_tracking_known = row
        .try_get::<i64, _>("review_tracking_known")
        .map_err(database_error)?;
    let reviewed_revision = row
        .try_get::<Option<i64>, _>("reviewed_revision")
        .map_err(database_error)?
        .map(Revision::new)
        .transpose()
        .map_err(|_| SqliteStoreError::CorruptData)?;
    let reviewed_at = row
        .try_get::<Option<i64>, _>("reviewed_at_ms")
        .map_err(database_error)?
        .map(UnixMillis::new);
    let reviewer_id = row
        .try_get::<Option<i64>, _>("reviewer_principal_id")
        .map_err(database_error)?;
    let reviewer_issuer = row
        .try_get::<Option<String>, _>("reviewer_issuer")
        .map_err(database_error)?;
    let reviewer_subject = row
        .try_get::<Option<String>, _>("reviewer_subject")
        .map_err(database_error)?;
    let review = match (
        review_tracking_known,
        reviewed_revision,
        reviewed_at,
        reviewer_id,
        reviewer_issuer,
        reviewer_subject,
    ) {
        (0, None, None, None, None, None) => NoteReviewTracking::Unknown,
        (1, None, None, None, None, None) => NoteReviewTracking::pending(),
        (1, Some(reviewed_revision), Some(reviewed_at), Some(id), Some(issuer), Some(subject)) => {
            let reviewer = PrincipalRef::new(
                PrincipalId::new(id).map_err(|_| SqliteStoreError::CorruptData)?,
                Identity::new(issuer, subject).map_err(|_| SqliteStoreError::CorruptData)?,
            );
            NoteReviewTracking::tracked(Some(NoteReviewRecord::new(
                reviewed_revision,
                reviewed_at,
                reviewer,
            )))
        }
        _ => return Err(SqliteStoreError::CorruptData),
    };
    let tags = serde_json::from_str(
        &row.try_get::<String, _>("tags_json")
            .map_err(database_error)?,
    )
    .map_err(|_| SqliteStoreError::CorruptData)?;
    let note = Note::restore(NoteRestore {
        note_id,
        owner,
        draft: NoteDraft {
            title: row.try_get("title").map_err(database_error)?,
            source: row.try_get("source").map_err(database_error)?,
            tags,
        },
        created_at: UnixMillis::new(row.try_get("created_at_ms").map_err(database_error)?),
        updated_at: changed_at,
        revision,
        deleted_at: row
            .try_get::<Option<i64>, _>("deleted_at_ms")
            .map_err(database_error)?
            .map(UnixMillis::new),
        created_via: row
            .try_get::<String, _>("created_via")
            .map_err(database_error)?
            .parse::<NoteCreationSource>()
            .map_err(|_| SqliteStoreError::CorruptData)?,
        review,
    })
    .map_err(|_| SqliteStoreError::CorruptData)?;
    let changed_by = principal_from_columns(
        &row,
        "changed_by_principal_id",
        "changed_by_issuer",
        "changed_by_subject",
    )?;
    let kind = row
        .try_get::<String, _>("change_kind")
        .map_err(database_error)?
        .parse()
        .map_err(|_| SqliteStoreError::CorruptData)?;
    Ok(NoteRevisionSnapshot::new(note, changed_by, kind))
}

fn principal_from_columns(
    row: &sqlx::sqlite::SqliteRow,
    id: &str,
    issuer: &str,
    subject: &str,
) -> Result<PrincipalRef, SqliteStoreError> {
    Ok(PrincipalRef::new(
        PrincipalId::new(row.try_get(id).map_err(database_error)?)
            .map_err(|_| SqliteStoreError::CorruptData)?,
        Identity::new(
            row.try_get(issuer).map_err(database_error)?,
            row.try_get(subject).map_err(database_error)?,
        )
        .map_err(|_| SqliteStoreError::CorruptData)?,
    ))
}
