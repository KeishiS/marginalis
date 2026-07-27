//! ノートの検証、可視性、revision規則を共有するuse case。

use async_trait::async_trait;
use marginalis_application::{Clock, NoteProfile, NoteUseCaseError, NoteUseCases, Random};
use marginalis_domain::{Actor, Note, NoteDraft, NoteId};
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
        SqliteStoreError::NotFound => NoteUseCaseError::NotFound,
        SqliteStoreError::Conflict => NoteUseCaseError::Conflict,
        SqliteStoreError::CorruptData => NoteUseCaseError::Unavailable,
        SqliteStoreError::ArchiveTargetNotEmpty | SqliteStoreError::Database(_) => {
            NoteUseCaseError::Unavailable
        }
    }
}

#[async_trait]
impl NoteUseCases for ServerNoteUseCases {
    async fn list_visible_notes(&self, actor: Actor) -> Result<Vec<Note>, NoteUseCaseError> {
        self.database
            .list_visible_notes(&actor)
            .await
            .map_err(map_note_error)
    }

    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.database
            .visible_note(&actor, note_id)
            .await
            .map_err(map_note_error)?
            .ok_or(NoteUseCaseError::NotFound)
    }

    async fn create_note(&self, actor: Actor, draft: NoteDraft) -> Result<Note, NoteUseCaseError> {
        let draft = marginalis_asciidoc::validate_note_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
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
            .create_note(&note)
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
        let draft = marginalis_asciidoc::validate_note_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        self.database
            .update_visible_note(
                &actor,
                note_id,
                expected_revision,
                &draft,
                SystemClock.now(),
            )
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
            .soft_delete_visible_note(&actor, note_id, expected_revision, SystemClock.now())
            .await
            .map_err(map_note_error)
    }

    async fn restore_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        self.database
            .restore_visible_note(&actor, note_id, expected_revision, SystemClock.now())
            .await
            .map_err(map_note_error)
    }

    fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        marginalis_asciidoc::export_note(note).map_err(|_| NoteUseCaseError::Unavailable)
    }

    fn render_note_html(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        marginalis_asciidoc::render_note_html(note).map_err(|_| NoteUseCaseError::Unavailable)
    }

    fn note_profile(&self) -> NoteProfile {
        marginalis_asciidoc::note_profile()
    }
}

#[cfg(test)]
mod tests {
    use marginalis_application::{
        NoteUseCaseError, NoteUseCases, NoteValidationCode, NoteValidationTarget,
    };
    use marginalis_domain::{Actor, NoteDraft};
    use marginalis_sqlite::SqliteDatabase;

    use super::ServerNoteUseCases;

    #[tokio::test]
    async fn owner_identity_hides_notes_from_other_identities() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let service = ServerNoteUseCases::new(database);
        let owner = Actor {
            issuer: "https://kanidm.example.test/oauth2/openid/marginalis".into(),
            subject: "owner".into(),
            is_administrator: false,
        };
        let reader = Actor {
            issuer: owner.issuer.clone(),
            subject: "reader".into(),
            is_administrator: false,
        };
        let same_subject_from_another_issuer = Actor {
            issuer: "https://other-kanidm.example.test/oauth2/openid/marginalis".into(),
            subject: owner.subject.clone(),
            is_administrator: false,
        };
        let note = service
            .create_note(
                owner.clone(),
                NoteDraft {
                    title: "SQLite canonical note".into(),
                    body: "Only SQLite persists this body.".into(),
                    tags: vec!["ownership".into(), "sqlite".into()],
                },
            )
            .await
            .expect("create");
        assert_eq!(note.creator_subject, "owner");
        assert_eq!(
            service.read_note(reader.clone(), note.note_id).await,
            Err(NoteUseCaseError::NotFound)
        );
        assert_eq!(
            service
                .read_note(same_subject_from_another_issuer, note.note_id)
                .await,
            Err(NoteUseCaseError::NotFound)
        );
        assert_eq!(
            service
                .update_note(
                    reader.clone(),
                    note.note_id,
                    NoteDraft {
                        title: "Hidden update".into(),
                        body: "Hidden body".into(),
                        tags: Vec::new(),
                    },
                    note.revision,
                )
                .await,
            Err(NoteUseCaseError::NotFound)
        );
        assert_eq!(
            service
                .update_note(
                    reader.clone(),
                    note.note_id,
                    NoteDraft {
                        title: "Hidden stale update".into(),
                        body: "Hidden stale body".into(),
                        tags: Vec::new(),
                    },
                    note.revision + 100,
                )
                .await,
            Err(NoteUseCaseError::NotFound)
        );
        assert_eq!(
            service
                .soft_delete_note(reader.clone(), note.note_id, note.revision)
                .await,
            Err(NoteUseCaseError::NotFound)
        );
        assert!(
            service
                .list_visible_notes(reader.clone())
                .await
                .expect("reader list")
                .is_empty()
        );
        let updated = service
            .update_note(
                owner.clone(),
                note.note_id,
                NoteDraft {
                    title: "Updated title".into(),
                    body: "Updated body".into(),
                    tags: vec!["sqlite".into()],
                },
                note.revision,
            )
            .await
            .expect("update");
        assert_eq!(updated.revision, note.revision + 1);
        let deleted = service
            .soft_delete_note(owner.clone(), note.note_id, updated.revision)
            .await
            .expect("soft delete");
        assert!(deleted.deleted_at.is_some());
        assert_eq!(
            service
                .restore_note(reader.clone(), note.note_id, deleted.revision)
                .await,
            Err(NoteUseCaseError::NotFound)
        );
        assert!(
            service
                .list_visible_notes(owner.clone())
                .await
                .expect("visible notes")
                .is_empty()
        );
        let restored = service
            .restore_note(owner, note.note_id, deleted.revision)
            .await
            .expect("restore");
        assert!(restored.deleted_at.is_none());
    }

    #[tokio::test]
    async fn validation_diagnostics_cross_the_use_case_boundary_without_loss() {
        let service = ServerNoteUseCases::new(
            SqliteDatabase::connect("sqlite::memory:")
                .await
                .expect("database"),
        );
        let error = service
            .create_note(
                Actor {
                    issuer: "https://id.example.test".into(),
                    subject: "writer".into(),
                    is_administrator: false,
                },
                NoteDraft {
                    title: String::new(),
                    body: "[source,brainfuck]\n----\n+\n----".into(),
                    tags: vec!["valid".into(), "bad,tag".into()],
                },
            )
            .await
            .expect_err("invalid draft");
        let NoteUseCaseError::Validation(diagnostics) = error else {
            panic!("expected validation diagnostics");
        };
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == NoteValidationCode::InvalidTitle
                && diagnostic.target == NoteValidationTarget::Title
                && diagnostic.span.is_none()
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == NoteValidationCode::InvalidTag
                && diagnostic.target == NoteValidationTarget::Tag { index: 1 }
                && diagnostic.span.is_none()
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == NoteValidationCode::UnsupportedSourceLanguage
                && diagnostic.target == NoteValidationTarget::Body
                && diagnostic.span.is_some()
        }));
    }
}
