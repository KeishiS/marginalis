//! ノートの検証、可視性、revision規則を共有するuse case。

use async_trait::async_trait;
use marginalis_application::{
    Clock, NoteProfile, NoteRenderContext, NoteUseCaseError, NoteUseCases, Random, RelatedNotes,
};
use marginalis_domain::{Actor, Note, NoteDraft, NoteId};
use marginalis_sqlite::{SqliteDatabase, SqliteStoreError};
use std::collections::HashSet;
use url::Url;

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

    async fn reference_resolutions(
        &self,
        actor: &Actor,
        note: &Note,
        context: &NoteRenderContext,
    ) -> Result<Vec<marginalis_asciidoc::NoteReferenceResolution>, NoteUseCaseError> {
        let queries = marginalis_asciidoc::note_reference_queries(note)
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        let mut resolutions = Vec::with_capacity(queries.len());
        for query in queries {
            let Some(target) = self
                .database
                .visible_note(actor, query.target_note_id)
                .await
                .map_err(map_note_error)?
            else {
                resolutions.push(marginalis_asciidoc::NoteReferenceResolution::Hidden {
                    reference_index: query.reference_index,
                });
                continue;
            };
            let missing_anchor = match query.anchor.as_deref() {
                Some(anchor) => !marginalis_asciidoc::note_has_anchor(&target, anchor)
                    .map_err(|_| NoteUseCaseError::Unavailable)?,
                None => false,
            };
            let href = note_href(
                &context.note_path_prefix,
                target.note_id,
                (!missing_anchor)
                    .then_some(query.anchor.as_deref())
                    .flatten(),
            )
            .ok_or(NoteUseCaseError::Unavailable)?;
            resolutions.push(marginalis_asciidoc::NoteReferenceResolution::Visible {
                reference_index: query.reference_index,
                href,
                title: target.title,
                missing_anchor,
            });
        }
        Ok(resolutions)
    }
}

fn note_href(prefix: &str, note_id: NoteId, anchor: Option<&str>) -> Option<String> {
    if !prefix.starts_with('/') || prefix.starts_with("//") || prefix.contains(['?', '#']) {
        return None;
    }
    let prefix = prefix.trim_end_matches('/');
    let path = format!("{prefix}/{note_id}");
    let mut url = Url::parse("https://marginalis.invalid")
        .ok()?
        .join(&path)
        .ok()?;
    if url.path() != path {
        return None;
    }
    url.set_fragment(anchor);
    Some(
        url.as_str()
            .strip_prefix("https://marginalis.invalid")?
            .to_owned(),
    )
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
        marginalis_domain::validate_identity(&actor.issuer, &actor.subject)
            .map_err(|_| NoteUseCaseError::Unavailable)?;
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

    async fn preview_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<String, NoteUseCaseError> {
        marginalis_domain::validate_identity(&actor.issuer, &actor.subject)
            .map_err(|_| NoteUseCaseError::Unavailable)?;
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
        let resolutions = self.reference_resolutions(&actor, &note, &context).await?;
        marginalis_asciidoc::render_note_html_with_references(&note, &resolutions)
            .map_err(|_| NoteUseCaseError::Unavailable)
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

    async fn render_note_html(
        &self,
        actor: Actor,
        note_id: NoteId,
        context: NoteRenderContext,
    ) -> Result<String, NoteUseCaseError> {
        let note = self.read_note(actor.clone(), note_id).await?;
        let resolutions = self.reference_resolutions(&actor, &note, &context).await?;
        marginalis_asciidoc::render_note_html_with_references(&note, &resolutions)
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn related_notes(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<RelatedNotes, NoteUseCaseError> {
        let source = self.read_note(actor.clone(), note_id).await?;
        let visible = self.list_visible_notes(actor).await?;
        let outgoing_ids = marginalis_asciidoc::note_reference_queries(&source)
            .map_err(|_| NoteUseCaseError::Unavailable)?
            .into_iter()
            .map(|query| query.target_note_id)
            .collect::<HashSet<_>>();

        let mut outgoing = Vec::new();
        let mut incoming = Vec::new();
        for candidate in visible {
            if outgoing_ids.contains(&candidate.note_id) {
                outgoing.push(candidate.clone());
            }
            let references_source = marginalis_asciidoc::note_reference_queries(&candidate)
                .map_err(|_| NoteUseCaseError::Unavailable)?
                .into_iter()
                .any(|query| query.target_note_id == source.note_id);
            if references_source {
                incoming.push(candidate);
            }
        }
        Ok(RelatedNotes { outgoing, incoming })
    }

    fn note_profile(&self) -> NoteProfile {
        marginalis_asciidoc::note_profile()
    }
}

#[cfg(test)]
mod tests {
    use marginalis_application::{
        NoteRenderContext, NoteUseCaseError, NoteUseCases, NoteValidationCode, NoteValidationTarget,
    };
    use marginalis_domain::{Actor, NoteDraft};
    use marginalis_sqlite::SqliteDatabase;

    use super::{ServerNoteUseCases, note_href};

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

    #[tokio::test]
    async fn preview_and_saved_note_use_the_same_html_rules() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let service = ServerNoteUseCases::new(database);
        let actor = Actor {
            issuer: "https://kanidm.example.test/oauth2/openid/marginalis".into(),
            subject: "previewer".into(),
            is_administrator: false,
        };
        let draft = NoteDraft {
            title: "プレビュー".into(),
            body: "日本語と絵文字😀を含む *本文*。".into(),
            tags: vec!["表示".into()],
        };

        let preview = service
            .preview_note(
                actor.clone(),
                draft.clone(),
                NoteRenderContext {
                    note_path_prefix: "/notes".into(),
                },
            )
            .await
            .expect("preview");
        let saved = service
            .create_note(actor.clone(), draft)
            .await
            .expect("create");

        assert_eq!(
            preview,
            service
                .render_note_html(
                    actor,
                    saved.note_id,
                    NoteRenderContext {
                        note_path_prefix: "/notes".into(),
                    },
                )
                .await
                .expect("saved HTML")
        );
    }

    #[tokio::test]
    async fn note_references_share_acl_anchor_and_subpath_rules() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let service = ServerNoteUseCases::new(database);
        let owner = Actor {
            issuer: "https://id.example.test".into(),
            subject: "owner".into(),
            is_administrator: false,
        };
        let other = Actor {
            issuer: owner.issuer.clone(),
            subject: "other".into(),
            is_administrator: false,
        };
        let target = service
            .create_note(
                owner.clone(),
                NoteDraft {
                    title: "非公開の参照先".into(),
                    body: "[[evidence]]\n根拠".into(),
                    tags: Vec::new(),
                },
            )
            .await
            .expect("target");
        let source = service
            .create_note(
                owner.clone(),
                NoteDraft {
                    title: "参照元".into(),
                    body: format!(
                        "xref:note:{}#evidence[] xref:note:{}#missing[欠落]",
                        target.note_id, target.note_id
                    ),
                    tags: Vec::new(),
                },
            )
            .await
            .expect("source");
        let context = NoteRenderContext {
            note_path_prefix: "/marginalis/notes".into(),
        };
        let html = service
            .render_note_html(owner.clone(), source.note_id, context.clone())
            .await
            .expect("HTML");
        assert!(html.contains(&format!(
            "href=\"/marginalis/notes/{}#evidence\"",
            target.note_id
        )));
        assert!(html.contains(&format!("href=\"/marginalis/notes/{}\"", target.note_id)));
        assert!(html.contains("非公開の参照先"));

        let other_source = service
            .create_note(
                other.clone(),
                NoteDraft {
                    title: "別利用者の参照元".into(),
                    body: format!("xref:note:{}[固定ラベル]", target.note_id),
                    tags: Vec::new(),
                },
            )
            .await
            .expect("other source");
        let hidden = service
            .render_note_html(other.clone(), other_source.note_id, context)
            .await
            .expect("hidden HTML");
        assert!(hidden.contains("固定ラベル"));
        assert!(!hidden.contains(&target.note_id.to_string()));
        assert!(!hidden.contains("非公開の参照先"));
        assert!(!hidden.contains("href="));

        let hidden_relations = service
            .related_notes(other, other_source.note_id)
            .await
            .expect("hidden relations");
        assert!(hidden_relations.outgoing.is_empty());
        assert_eq!(
            service
                .related_notes(owner.clone(), target.note_id)
                .await
                .expect("owner relations")
                .incoming
                .iter()
                .map(|note| note.note_id)
                .collect::<Vec<_>>(),
            vec![source.note_id]
        );
        let administrator = Actor {
            issuer: owner.issuer,
            subject: "administrator".into(),
            is_administrator: true,
        };
        let administrator_relations = service
            .related_notes(administrator, target.note_id)
            .await
            .expect("administrator relations");
        assert_eq!(administrator_relations.incoming.len(), 2);
        assert!(
            administrator_relations
                .incoming
                .iter()
                .any(|note| note.note_id == other_source.note_id)
        );
    }

    #[tokio::test]
    async fn related_notes_are_deduplicated_and_follow_current_note_state() {
        let database = SqliteDatabase::connect("sqlite::memory:")
            .await
            .expect("database");
        let service = ServerNoteUseCases::new(database);
        let owner = Actor {
            issuer: "https://id.example.test".into(),
            subject: "owner".into(),
            is_administrator: false,
        };
        let target = service
            .create_note(
                owner.clone(),
                NoteDraft {
                    title: "参照先".into(),
                    body: "本文".into(),
                    tags: vec!["z".into(), "a".into()],
                },
            )
            .await
            .expect("target");
        let source = service
            .create_note(
                owner.clone(),
                NoteDraft {
                    title: "参照元".into(),
                    body: format!(
                        "xref:note:{}[一つ目]\n\nxref:note:{}[二つ目]",
                        target.note_id, target.note_id
                    ),
                    tags: Vec::new(),
                },
            )
            .await
            .expect("source");

        let source_related = service
            .related_notes(owner.clone(), source.note_id)
            .await
            .expect("source relations");
        assert_eq!(
            source_related
                .outgoing
                .iter()
                .map(|note| note.note_id)
                .collect::<Vec<_>>(),
            vec![target.note_id]
        );
        assert!(source_related.incoming.is_empty());

        let target_related = service
            .related_notes(owner.clone(), target.note_id)
            .await
            .expect("target relations");
        assert_eq!(
            target_related
                .incoming
                .iter()
                .map(|note| note.note_id)
                .collect::<Vec<_>>(),
            vec![source.note_id]
        );

        service
            .update_note(
                owner.clone(),
                source.note_id,
                NoteDraft {
                    title: source.title,
                    body: "参照を削除しました。".into(),
                    tags: source.tags,
                },
                source.revision,
            )
            .await
            .expect("update source");
        assert!(
            service
                .related_notes(owner, target.note_id)
                .await
                .expect("updated relations")
                .incoming
                .is_empty()
        );
    }

    #[test]
    fn note_href_rejects_unsafe_prefixes_and_encodes_fragments() {
        let note_id = marginalis_domain::NoteId::new(
            "0197c9bc-0000-7000-8000-000000000001"
                .parse()
                .expect("UUIDv7"),
        );
        assert_eq!(
            note_href("/marginalis/notes", note_id, Some("日本 語")),
            Some(format!(
                "/marginalis/notes/{note_id}#%E6%97%A5%E6%9C%AC%20%E8%AA%9E"
            ))
        );
        for prefix in [
            "https://example.test/notes",
            "//example.test",
            "/notes?x=1",
            "/a/../notes",
        ] {
            assert_eq!(note_href(prefix, note_id, None), None, "{prefix}");
        }
    }
}
