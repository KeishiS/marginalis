use std::{
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use marginalis_domain::{
    Actor, BibliographyItem, DeletedNoteListEntry, EntityId, Identity, Note, NoteAccess,
    NoteAclEntry, NoteDraft, NoteId, NoteListEntry, NoteRestore, NoteReviewRecord,
    NoteReviewTracking, NoteSummary, NoteValidationTarget, Revision, UnixMillis, Utf8ByteSpan,
};

use crate::{
    BibliographyRepository, CitationStyle, Clock, MathMacro, MathMacroRepository,
    MathMacroSettings, NoteAclState, NoteAdvisoryDiagnostic, NoteAdvisorySeverity, NoteProfile,
    NoteRenderContext, NoteValidationDiagnostic, Random, StorageError, ValidatedNoteDraft,
};

use super::{
    AccessibleNote, NoteAclRepository, NoteApplication, NoteApplicationDependencies,
    NoteCitationQuery, NoteCommandRepository, NoteContent, NoteContentError, NoteGraph,
    NoteGraphQuery, NoteLinkResolver, NoteLinks, NoteQueryRepository, NoteReferenceQuery,
    NoteRenderInputs, NoteReviewRepository, NoteViewSnapshot,
};

/// 決定的なclockと乱数を使い、repository4種を同じ`MemoryNotes`が担う試験用のservice。
pub(super) fn note_application(
    repository: &Arc<MemoryNotes>,
    content: Arc<dyn NoteContent>,
    bibliography: Arc<dyn BibliographyRepository>,
    math_macros: Arc<dyn MathMacroRepository>,
) -> NoteApplication {
    NoteApplication::new(NoteApplicationDependencies {
        queries: repository.clone(),
        commands: repository.clone(),
        access_control: repository.clone(),
        reviews: repository.clone(),
        content,
        bibliography,
        math_macros,
        links: Arc::new(NoLinks),
        clock: Arc::new(FixedClock),
        random: Arc::new(FixedRandom),
    })
}

pub(super) struct MemoryNotes {
    pub(super) notes: Mutex<Vec<Note>>,
    pub(super) update_calls: AtomicUsize,
    pub(super) accessible_as: Mutex<Option<NoteAccess>>,
}

impl Default for MemoryNotes {
    fn default() -> Self {
        Self {
            notes: Mutex::new(Vec::new()),
            update_calls: AtomicUsize::new(0),
            accessible_as: Mutex::new(Some(NoteAccess::Manage)),
        }
    }
}

#[async_trait]
impl NoteQueryRepository for MemoryNotes {
    async fn list_visible_notes(
        &self,
        _actor: &Actor,
        _query: &crate::NoteListQuery,
    ) -> Result<Vec<NoteListEntry>, StorageError> {
        Ok(self
            .notes
            .lock()
            .expect("notes lock")
            .iter()
            .map(|note| NoteListEntry {
                summary: NoteSummary::from(note),
                access: NoteAccess::Manage,
            })
            .collect())
    }

    async fn list_owned_deleted_notes(
        &self,
        _actor: &Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, StorageError> {
        Ok(Vec::new())
    }

    async fn accessible_note(
        &self,
        _actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<AccessibleNote>, StorageError> {
        let access = *self.accessible_as.lock().expect("access lock");
        Ok(access.and_then(|access| {
            self.notes
                .lock()
                .expect("notes lock")
                .iter()
                .find(|note| note.note_id() == note_id)
                .cloned()
                .map(|note| AccessibleNote { note, access })
        }))
    }

    async fn visible_notes_by_id(
        &self,
        actor: &Actor,
        note_ids: &[NoteId],
    ) -> Result<Vec<Note>, StorageError> {
        let mut notes = Vec::new();
        for note_id in note_ids {
            if let Some(accessible) = self.accessible_note(actor, *note_id).await? {
                notes.push(accessible.note);
            }
        }
        Ok(notes)
    }

    async fn note_view_snapshot(
        &self,
        _actor: &Actor,
        _note_id: NoteId,
    ) -> Result<Option<NoteViewSnapshot>, StorageError> {
        Ok(None)
    }

    async fn note_graph(
        &self,
        _actor: &Actor,
        _query: &NoteGraphQuery,
    ) -> Result<NoteGraph, StorageError> {
        Ok(NoteGraph::default())
    }
}

#[async_trait]
impl NoteCommandRepository for MemoryNotes {
    async fn create_note(&self, note: &Note, _links: NoteLinks<'_>) -> Result<(), StorageError> {
        self.notes.lock().expect("notes lock").push(note.clone());
        Ok(())
    }

    async fn update_visible_note(
        &self,
        _actor: &Actor,
        _note_id: NoteId,
        _expected_revision: Revision,
        _draft: &NoteDraft,
        _links: NoteLinks<'_>,
        _now: UnixMillis,
    ) -> Result<Note, StorageError> {
        self.update_calls.fetch_add(1, Ordering::Relaxed);
        Err(StorageError::Unavailable)
    }

    async fn soft_delete_visible_note(
        &self,
        _actor: &Actor,
        _note_id: NoteId,
        _expected_revision: Revision,
        _now: UnixMillis,
    ) -> Result<Note, StorageError> {
        Err(StorageError::Unavailable)
    }

    async fn restore_owned_deleted_note(
        &self,
        _actor: &Actor,
        _note_id: NoteId,
        _expected_revision: Revision,
        _now: UnixMillis,
    ) -> Result<Note, StorageError> {
        Err(StorageError::Unavailable)
    }
}

#[async_trait]
impl NoteAclRepository for MemoryNotes {
    async fn read_note_acl(
        &self,
        _actor: &Actor,
        _note_id: NoteId,
    ) -> Result<NoteAclState, StorageError> {
        Ok(NoteAclState {
            entries: Vec::new(),
            revision: Revision::INITIAL,
        })
    }

    async fn replace_note_acl(
        &self,
        _actor: &Actor,
        _note_id: NoteId,
        _entries: &[NoteAclEntry],
        _expected_revision: Revision,
        _now: UnixMillis,
    ) -> Result<Note, StorageError> {
        Err(StorageError::Unavailable)
    }
}

#[async_trait]
impl NoteReviewRepository for MemoryNotes {
    async fn read_owned_note_review(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Note, StorageError> {
        self.accessible_note(actor, note_id)
            .await?
            .filter(|accessible| {
                accessible.access == NoteAccess::Manage
                    && accessible.note.owner() == actor.identity()
                    && accessible.note.deleted_at().is_none()
            })
            .map(|accessible| accessible.note)
            .ok_or(StorageError::NotFound)
    }

    async fn mark_owned_note_reviewed(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        reviewed_at: UnixMillis,
    ) -> Result<Note, StorageError> {
        let current = self.read_owned_note_review(actor, note_id).await?;
        if current.revision() != expected_revision {
            return Err(StorageError::Conflict);
        }
        let next_revision =
            Revision::new(current.revision().get() + 1).map_err(|_| StorageError::CorruptData)?;
        let reviewed = Note::restore(NoteRestore {
            note_id,
            owner: current.owner().clone(),
            draft: NoteDraft {
                source: current.source().to_owned(),
                title: current.title().to_owned(),
                tags: current.tags().to_vec(),
            },
            created_at: current.created_at(),
            updated_at: reviewed_at,
            revision: next_revision,
            deleted_at: current.deleted_at(),
            created_via: current.created_via(),
            review: NoteReviewTracking::tracked(Some(NoteReviewRecord::new(
                next_revision,
                reviewed_at,
                actor.identity().clone(),
            ))),
        })
        .map_err(|_| StorageError::CorruptData)?;
        let mut notes = self.notes.lock().expect("notes lock");
        let stored = notes
            .iter_mut()
            .find(|note| note.note_id() == note_id)
            .ok_or(StorageError::NotFound)?;
        *stored = reviewed.clone();
        Ok(reviewed)
    }
}

#[derive(Default)]
pub(super) struct AcceptContent {
    pub(super) reference_query_calls: AtomicUsize,
}

impl NoteContent for AcceptContent {
    fn validate_draft(
        &self,
        draft: NoteDraft,
    ) -> Result<ValidatedNoteDraft, Vec<NoteValidationDiagnostic>> {
        Ok(ValidatedNoteDraft {
            draft,
            diagnostics: vec![NoteAdvisoryDiagnostic {
                code: "test-advisory".into(),
                severity: NoteAdvisorySeverity::Warning,
                target: NoteValidationTarget::Source,
                span: None,
                message: "test advisory".into(),
            }],
            reference_queries: Vec::new(),
            citation_queries: Vec::new(),
            citation_style: CitationStyle::default(),
        })
    }

    fn reference_queries(&self, _body: &str) -> Result<Vec<NoteReferenceQuery>, NoteContentError> {
        self.reference_query_calls.fetch_add(1, Ordering::Relaxed);
        Ok(Vec::new())
    }

    fn citation_queries(&self, _body: &str) -> Result<Vec<NoteCitationQuery>, NoteContentError> {
        Ok(Vec::new())
    }

    fn citation_style(&self, _body: &str) -> Result<CitationStyle, NoteContentError> {
        Ok(CitationStyle::default())
    }

    fn has_anchor(&self, _body: &str, _anchor: &str) -> Result<bool, NoteContentError> {
        Ok(false)
    }

    fn render(
        &self,
        _note: &Note,
        _inputs: NoteRenderInputs<'_>,
    ) -> Result<String, NoteContentError> {
        Ok("<article><p>preview</p></article>".into())
    }

    fn export(&self, _note: &Note) -> Result<String, NoteContentError> {
        Ok(String::new())
    }

    fn profile(&self) -> NoteProfile {
        unreachable!("this test does not read the authoring profile")
    }
}

pub(super) struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> UnixMillis {
        UnixMillis::new(1_700_000_000_000)
    }
}

pub(super) struct FixedRandom;

impl Random for FixedRandom {
    fn uuid_v7(&self) -> EntityId {
        EntityId::from_str("01890f3c-6a4d-7cc2-98b3-84b68f68c6e1").expect("fixed UUIDv7")
    }

    fn opaque_token(&self) -> String {
        unreachable!("note creation does not issue an opaque token")
    }
}

/// 引用のないノートだけを扱う試験用の書誌ライブラリー。
pub(super) struct EmptyLibrary;

#[async_trait]
impl BibliographyRepository for EmptyLibrary {
    async fn search_owned_items(
        &self,
        _actor: &Actor,
        _query: &str,
    ) -> Result<Vec<BibliographyItem>, StorageError> {
        Ok(Vec::new())
    }

    async fn items_by_citation_keys(
        &self,
        _owner: &Identity,
        _citation_keys: &[String],
    ) -> Result<Vec<BibliographyItem>, StorageError> {
        Ok(Vec::new())
    }

    async fn create_owned_item(&self, _item: &BibliographyItem) -> Result<(), StorageError> {
        unreachable!("this test does not write bibliography items")
    }

    async fn update_owned_item(
        &self,
        _actor: &Actor,
        _item_id: marginalis_domain::BibliographyItemId,
        _citation_key: &str,
        _csl_json: &str,
        _updated_at: UnixMillis,
        _expected_revision: Revision,
    ) -> Result<BibliographyItem, StorageError> {
        unreachable!("this test does not write bibliography items")
    }

    async fn delete_owned_item(
        &self,
        _actor: &Actor,
        _item_id: marginalis_domain::BibliographyItemId,
        _expected_revision: Revision,
    ) -> Result<(), StorageError> {
        unreachable!("this test does not write bibliography items")
    }
}

pub(super) struct NoLinks;

impl NoteLinkResolver for NoLinks {
    fn href(
        &self,
        _context: &NoteRenderContext,
        _note_id: NoteId,
        _anchor: Option<&str>,
    ) -> Option<String> {
        None
    }
}

pub(super) struct NoMathMacros;

#[async_trait]
impl MathMacroRepository for NoMathMacros {
    async fn read_math_macros(&self, _owner: &Identity) -> Result<MathMacroSettings, StorageError> {
        Ok(MathMacroSettings::default())
    }

    async fn replace_math_macros(
        &self,
        _owner: &Identity,
        _macros: &[MathMacro],
        _expected_revision: i64,
    ) -> Result<MathMacroSettings, StorageError> {
        unreachable!("note tests do not replace MathJax macros")
    }
}

pub(super) struct OwnerMathMacros;

#[async_trait]
impl MathMacroRepository for OwnerMathMacros {
    async fn read_math_macros(&self, owner: &Identity) -> Result<MathMacroSettings, StorageError> {
        Ok(MathMacroSettings {
            macros: (owner == &OneItemLibrary::owner())
                .then(|| MathMacro {
                    name: "bm".into(),
                    replacement: r"\boldsymbol{#1}".into(),
                    argument_count: 1,
                })
                .into_iter()
                .collect(),
            revision: 1,
        })
    }

    async fn replace_math_macros(
        &self,
        _owner: &Identity,
        _macros: &[MathMacro],
        _expected_revision: i64,
    ) -> Result<MathMacroSettings, StorageError> {
        unreachable!("note tests do not replace MathJax macros")
    }
}

/// 1件だけ登録された書誌ライブラリー。所有者が一致する問い合わせにだけ答える。
pub(super) struct OneItemLibrary;

impl OneItemLibrary {
    pub(super) fn owner() -> Identity {
        Identity::new("https://id.example.test".into(), "alice".into()).expect("owner")
    }

    fn item() -> BibliographyItem {
        BibliographyItem::create(
            marginalis_domain::BibliographyItemId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000021").expect("UUIDv7"),
            ),
            &Self::owner(),
            "smith2024".into(),
            serde_json::json!({
                "id": "smith2024",
                "type": "article-journal",
                "title": "An Example Article",
                "author": [{ "family": "Smith", "given": "Alex" }],
                "issued": { "date-parts": [[2024]] }
            })
            .to_string(),
            UnixMillis::new(0),
        )
    }

    /// 番号で示すスタイルの通し番号を確かめるための2件目。
    fn second_item() -> BibliographyItem {
        BibliographyItem::create(
            marginalis_domain::BibliographyItemId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000022").expect("UUIDv7"),
            ),
            &Self::owner(),
            "tanaka2025".into(),
            serde_json::json!({
                "id": "tanaka2025",
                "type": "article-journal",
                "title": "追試の報告",
                "author": [{ "family": "Tanaka", "given": "Bun" }],
                "issued": { "date-parts": [[2025]] }
            })
            .to_string(),
            UnixMillis::new(0),
        )
    }
}

#[async_trait]
impl BibliographyRepository for OneItemLibrary {
    async fn search_owned_items(
        &self,
        _actor: &Actor,
        _query: &str,
    ) -> Result<Vec<BibliographyItem>, StorageError> {
        Ok(Vec::new())
    }

    async fn items_by_citation_keys(
        &self,
        owner: &Identity,
        citation_keys: &[String],
    ) -> Result<Vec<BibliographyItem>, StorageError> {
        if owner != &Self::owner() {
            return Ok(Vec::new());
        }
        Ok([Self::item(), Self::second_item()]
            .into_iter()
            .filter(|item| citation_keys.iter().any(|key| key == item.citation_key()))
            .collect())
    }

    async fn create_owned_item(&self, _item: &BibliographyItem) -> Result<(), StorageError> {
        unreachable!("this test does not write bibliography items")
    }

    async fn update_owned_item(
        &self,
        _actor: &Actor,
        _item_id: marginalis_domain::BibliographyItemId,
        _citation_key: &str,
        _csl_json: &str,
        _updated_at: UnixMillis,
        _expected_revision: Revision,
    ) -> Result<BibliographyItem, StorageError> {
        unreachable!("this test does not write bibliography items")
    }

    async fn delete_owned_item(
        &self,
        _actor: &Actor,
        _item_id: marginalis_domain::BibliographyItemId,
        _expected_revision: Revision,
    ) -> Result<(), StorageError> {
        unreachable!("this test does not write bibliography items")
    }
}

/// 引用だけを報告し、他の診断を出さない試験用の文書adapter。
pub(super) struct CitingContent {
    pub(super) keys: Vec<String>,
}

impl NoteContent for CitingContent {
    fn validate_draft(
        &self,
        draft: NoteDraft,
    ) -> Result<ValidatedNoteDraft, Vec<NoteValidationDiagnostic>> {
        Ok(ValidatedNoteDraft {
            draft,
            diagnostics: Vec::new(),
            reference_queries: Vec::new(),
            citation_queries: vec![NoteCitationQuery {
                citation_index: 0,
                keys: self.keys.clone(),
                locator: None,
                span: Utf8ByteSpan { start: 0, end: 1 },
            }],
            citation_style: CitationStyle::default(),
        })
    }

    fn reference_queries(&self, _body: &str) -> Result<Vec<NoteReferenceQuery>, NoteContentError> {
        Ok(Vec::new())
    }

    fn citation_queries(&self, _body: &str) -> Result<Vec<NoteCitationQuery>, NoteContentError> {
        Ok(Vec::new())
    }

    fn citation_style(&self, _body: &str) -> Result<CitationStyle, NoteContentError> {
        Ok(CitationStyle::default())
    }

    fn has_anchor(&self, _body: &str, _anchor: &str) -> Result<bool, NoteContentError> {
        Ok(false)
    }

    fn render(
        &self,
        _note: &Note,
        _inputs: NoteRenderInputs<'_>,
    ) -> Result<String, NoteContentError> {
        Ok(String::new())
    }

    fn export(&self, _note: &Note) -> Result<String, NoteContentError> {
        Ok(String::new())
    }

    fn profile(&self) -> NoteProfile {
        unreachable!("this test does not read the authoring profile")
    }
}
