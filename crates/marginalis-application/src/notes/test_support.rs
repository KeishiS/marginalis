use std::{
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use marginalis_domain::{
    ATTACHMENT_POLICY, Actor, AttachmentId, AttachmentMetadata, BibliographyItem,
    DeletedNoteListEntry, EntityId, Identity, Note, NoteAccess, NoteAclEntry, NoteDraft, NoteId,
    NoteListEntry, NoteRestore, NoteReviewRecord, NoteReviewTracking, NoteRevisionKind,
    NoteRevisionSnapshot, NoteRevisionSummary, NoteSummary, NoteValidationTarget, PrincipalId,
    PrincipalRef, Revision, StoredAttachment, UnixMillis, Utf8ByteSpan,
};

use crate::{
    BibliographyRepository, CitationStyle, Clock, MathMacro, MathMacroRepository,
    MathMacroSettings, NoteAclState, NoteAdvisoryDiagnostic, NoteAdvisorySeverity, NoteProfile,
    NoteRenderContext, NoteValidationDiagnostic, PrincipalDirectory, Random, StorageError,
    ValidatedNoteDraft,
};

use super::{
    AccessibleNote, NoteAclRepository, NoteApplication, NoteApplicationDependencies,
    NoteCitationQuery, NoteCommandRepository, NoteContent, NoteContentError, NoteGraph,
    NoteGraphQuery, NoteLinkResolver, NoteLinks, NoteQueryRepository, NoteReferenceQuery,
    NoteRenderInputs, NoteReviewRepository, NoteRevisionView, NoteSyncPage, NoteSyncRepository,
    NoteSyncRepositoryError, NoteViewSnapshot,
};

/// 決定的なclockと乱数を使い、ノート保存の全機能を同じ`MemoryNotes`が担う試験用service。
pub(super) fn note_application(
    repository: &Arc<MemoryNotes>,
    content: Arc<dyn NoteContent>,
    bibliography: Arc<dyn BibliographyRepository>,
    math_macros: Arc<dyn MathMacroRepository>,
) -> NoteApplication {
    note_application_with_links(
        repository,
        content,
        bibliography,
        math_macros,
        Arc::new(NoLinks),
    )
}

pub(super) fn note_application_with_links(
    repository: &Arc<MemoryNotes>,
    content: Arc<dyn NoteContent>,
    bibliography: Arc<dyn BibliographyRepository>,
    math_macros: Arc<dyn MathMacroRepository>,
    links: Arc<dyn NoteLinkResolver>,
) -> NoteApplication {
    NoteApplication::new(NoteApplicationDependencies {
        notes: repository.clone(),
        content,
        bibliography,
        math_macros,
        links,
        principals: Arc::new(TestPrincipalDirectory),
        acl_issuer: "https://id.example.test".into(),
        clock: Arc::new(FixedClock),
        random: Arc::new(FixedRandom),
    })
}

#[async_trait]
impl NoteSyncRepository for MemoryNotes {
    async fn sync_notes(
        &self,
        _actor: &Actor,
        _cursor: Option<&str>,
        _limit: usize,
        _next_cursor: &str,
        _now: UnixMillis,
    ) -> Result<NoteSyncPage, NoteSyncRepositoryError> {
        Err(NoteSyncRepositoryError::Storage(StorageError::Unavailable))
    }
}

pub(super) struct MemoryNotes {
    pub(super) notes: Mutex<Vec<Note>>,
    pub(super) histories: Mutex<Vec<NoteRevisionSnapshot>>,
    pub(super) attachments: Mutex<Vec<StoredAttachment>>,
    pub(super) update_calls: AtomicUsize,
    pub(super) accessible_as: Mutex<Option<NoteAccess>>,
}

impl Default for MemoryNotes {
    fn default() -> Self {
        Self {
            notes: Mutex::new(Vec::new()),
            histories: Mutex::new(Vec::new()),
            attachments: Mutex::new(Vec::new()),
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

    async fn list_note_revisions(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Vec<NoteRevisionSummary>>, StorageError> {
        let visible = self.accessible_note(actor, note_id).await?.is_some();
        if !visible {
            return Ok(None);
        }
        let mut revisions = self
            .histories
            .lock()
            .expect("history lock")
            .iter()
            .filter(|entry| entry.note().note_id() == note_id)
            .map(NoteRevisionSummary::from)
            .collect::<Vec<_>>();
        revisions.sort_by_key(|entry| std::cmp::Reverse(entry.revision));
        Ok((!revisions.is_empty()).then_some(revisions))
    }

    async fn note_revision(
        &self,
        actor: &Actor,
        note_id: NoteId,
        revision: Revision,
    ) -> Result<Option<NoteRevisionView>, StorageError> {
        let access = *self.accessible_as.lock().expect("access lock");
        if self.accessible_note(actor, note_id).await?.is_none() {
            return Ok(None);
        }
        Ok(self
            .histories
            .lock()
            .expect("history lock")
            .iter()
            .find(|entry| entry.note().note_id() == note_id && entry.note().revision() == revision)
            .cloned()
            .map(|revision| NoteRevisionView {
                revision,
                access: access.expect("visible history has access"),
            }))
    }

    async fn list_note_attachments(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Vec<AttachmentMetadata>>, StorageError> {
        if self.accessible_note(actor, note_id).await?.is_none() {
            return Ok(None);
        }
        Ok(Some(
            self.attachments
                .lock()
                .expect("attachments lock")
                .iter()
                .filter(|entry| entry.metadata().note_id() == note_id)
                .map(|entry| entry.metadata().clone())
                .collect(),
        ))
    }

    async fn note_attachment(
        &self,
        actor: &Actor,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Result<Option<StoredAttachment>, StorageError> {
        if self.accessible_note(actor, note_id).await?.is_none() {
            return Ok(None);
        }
        Ok(self
            .attachments
            .lock()
            .expect("attachments lock")
            .iter()
            .find(|entry| {
                entry.metadata().note_id() == note_id
                    && entry.metadata().attachment_id() == attachment_id
            })
            .cloned())
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
        self.histories
            .lock()
            .expect("history lock")
            .push(NoteRevisionSnapshot::new(
                note.clone(),
                note.owner().clone(),
                NoteRevisionKind::Created,
            ));
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

    async fn restore_visible_note_revision(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        draft: &NoteDraft,
        links: NoteLinks<'_>,
        now: UnixMillis,
    ) -> Result<Note, StorageError> {
        let _ = links;
        let mut notes = self.notes.lock().expect("notes lock");
        let current = notes
            .iter_mut()
            .find(|note| note.note_id() == note_id)
            .ok_or(StorageError::NotFound)?;
        if current.revision() != expected_revision {
            return Err(StorageError::Conflict);
        }
        let next_revision =
            Revision::new(current.revision().get() + 1).map_err(|_| StorageError::CorruptData)?;
        let review = if current.review_tracking_known() {
            NoteReviewTracking::tracked(current.last_review().cloned())
        } else {
            NoteReviewTracking::Unknown
        };
        let restored = Note::restore(NoteRestore {
            note_id,
            owner: current.owner().clone(),
            draft: draft.clone(),
            created_at: current.created_at(),
            updated_at: now,
            revision: next_revision,
            deleted_at: None,
            created_via: current.created_via(),
            review,
        })
        .map_err(|_| StorageError::CorruptData)?;
        *current = restored.clone();
        drop(notes);
        self.histories
            .lock()
            .expect("history lock")
            .push(NoteRevisionSnapshot::new(
                restored.clone(),
                actor.principal().clone(),
                NoteRevisionKind::HistoryRestored,
            ));
        Ok(restored)
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

    async fn create_note_attachment(
        &self,
        actor: &Actor,
        attachment: &StoredAttachment,
    ) -> Result<(), StorageError> {
        let accessible = self
            .accessible_note(actor, attachment.metadata().note_id())
            .await?
            .filter(|entry| entry.access.allows(NoteAccess::Edit))
            .ok_or(StorageError::NotFound)?;
        if accessible.note.deleted_at().is_some() {
            return Err(StorageError::NotFound);
        }
        let mut attachments = self.attachments.lock().expect("attachments lock");
        let current = attachments
            .iter()
            .filter(|entry| entry.metadata().note_id() == accessible.note.note_id())
            .collect::<Vec<_>>();
        let total = current
            .iter()
            .map(|entry| entry.metadata().byte_length())
            .sum::<usize>();
        if current.len() >= ATTACHMENT_POLICY.max_attachments_per_note
            || total.saturating_add(attachment.metadata().byte_length())
                > ATTACHMENT_POLICY.max_bytes_per_note
        {
            return Err(StorageError::Conflict);
        }
        attachments.push(attachment.clone());
        Ok(())
    }

    async fn delete_unused_note_attachment(
        &self,
        actor: &Actor,
        note_id: NoteId,
        attachment_id: AttachmentId,
    ) -> Result<(), StorageError> {
        let accessible = self
            .accessible_note(actor, note_id)
            .await?
            .filter(|entry| entry.access.allows(NoteAccess::Edit))
            .ok_or(StorageError::NotFound)?;
        if accessible.note.deleted_at().is_some() {
            return Err(StorageError::NotFound);
        }
        let mut attachments = self.attachments.lock().expect("attachments lock");
        let Some(position) = attachments.iter().position(|entry| {
            entry.metadata().note_id() == note_id
                && entry.metadata().attachment_id() == attachment_id
        }) else {
            return Err(StorageError::NotFound);
        };
        attachments.remove(position);
        Ok(())
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
                    && accessible.note.owner() == actor.principal()
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
                actor.principal().clone(),
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
                position: None,
                message: "test advisory".into(),
            }],
            reference_queries: Vec::new(),
            citation_queries: Vec::new(),
            attachment_queries: Vec::new(),
            citation_style: CitationStyle::default(),
            source_spans: Vec::new(),
        })
    }

    fn reference_queries(&self, _body: &str) -> Result<Vec<NoteReferenceQuery>, NoteContentError> {
        self.reference_query_calls.fetch_add(1, Ordering::Relaxed);
        Ok(Vec::new())
    }

    fn citation_queries(&self, _body: &str) -> Result<Vec<NoteCitationQuery>, NoteContentError> {
        Ok(Vec::new())
    }

    fn attachment_queries(
        &self,
        _body: &str,
    ) -> Result<Vec<crate::NoteAttachmentQuery>, NoteContentError> {
        Ok(Vec::new())
    }

    fn citation_style(&self, _body: &str) -> Result<CitationStyle, NoteContentError> {
        Ok(CitationStyle::default())
    }

    fn outline(&self, body: &str) -> Result<crate::NoteOutline, NoteContentError> {
        Ok(crate::NoteOutline {
            sections: Vec::new(),
            line_count: body.lines().count(),
        })
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
        NoteProfile {
            profile_version: 1,
            adocweave_package_version: "test",
            limits: crate::NoteProfileLimits {
                max_title_characters: 1,
                max_source_bytes: 1,
                max_patch_bytes: 1,
                max_patch_hunks: 1,
                max_tags: 1,
                max_tag_characters: 1,
                max_attachment_bytes: 1,
                max_attachments_per_note: 1,
                max_attachment_bytes_per_note: 1,
                max_attachment_file_name_characters: 1,
            },
            normalization: crate::NoteProfileNormalization {
                title: Vec::new(),
                tags: Vec::new(),
            },
            syntax: crate::NoteProfileSyntax {
                common_blocks: Vec::new(),
                common_inlines: Vec::new(),
                source_language_optional: true,
                allowed_math_languages: Vec::new(),
                allowed_document_attributes: Vec::new(),
                allowed_citation_styles: Vec::new(),
                title_forbidden: Vec::new(),
                tag_forbidden: Vec::new(),
            },
            authoring_guidance: Vec::new(),
            allowed_source_languages: Vec::new(),
            forbidden_rules: Vec::new(),
            advisory_rules: Vec::new(),
            examples: Vec::new(),
        }
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

pub(super) fn identity(subject: &str) -> Identity {
    Identity::new("https://id.example.test".into(), subject.into()).expect("valid identity")
}

pub(super) fn principal(subject: &str, id: i64) -> PrincipalRef {
    PrincipalRef::new(
        PrincipalId::new(id).expect("positive principal ID"),
        identity(subject),
    )
}

pub(super) fn actor(subject: &str, id: i64) -> Actor {
    Actor::for_single_identity(
        PrincipalId::new(id).expect("positive principal ID"),
        identity(subject),
    )
}

pub(super) struct TestPrincipalDirectory;

#[async_trait]
impl PrincipalDirectory for TestPrincipalDirectory {
    async fn resolve_or_create_verified(&self, identity: Identity) -> Result<Actor, StorageError> {
        Ok(Actor::for_single_identity(
            PrincipalId::new(1).expect("ID"),
            identity,
        ))
    }

    async fn resolve(&self, identity: &Identity) -> Result<Option<Actor>, StorageError> {
        Ok(Some(Actor::for_single_identity(
            PrincipalId::new(1).expect("ID"),
            identity.clone(),
        )))
    }

    async fn resolve_or_create_acl_target(
        &self,
        identity: Identity,
    ) -> Result<PrincipalRef, StorageError> {
        Ok(PrincipalRef::new(
            PrincipalId::new(2).expect("ID"),
            identity,
        ))
    }
}

/// 引用のないノートだけを扱う試験用の文献ライブラリ。
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
        _owner: &PrincipalRef,
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
        _csl_json: &marginalis_domain::ValidatedCslJson,
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

    fn attachment_href(
        &self,
        _context: &NoteRenderContext,
        _note_id: NoteId,
        _attachment_id: AttachmentId,
    ) -> Option<String> {
        None
    }
}

pub(super) struct NoMathMacros;

#[async_trait]
impl MathMacroRepository for NoMathMacros {
    async fn read_math_macros(
        &self,
        _owner: &PrincipalRef,
    ) -> Result<MathMacroSettings, StorageError> {
        Ok(MathMacroSettings::default())
    }

    async fn replace_math_macros(
        &self,
        _owner: &PrincipalRef,
        _macros: &[MathMacro],
        _expected_revision: i64,
    ) -> Result<MathMacroSettings, StorageError> {
        unreachable!("note tests do not replace MathJax macros")
    }
}

pub(super) struct OwnerMathMacros;

#[async_trait]
impl MathMacroRepository for OwnerMathMacros {
    async fn read_math_macros(
        &self,
        owner: &PrincipalRef,
    ) -> Result<MathMacroSettings, StorageError> {
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
        _owner: &PrincipalRef,
        _macros: &[MathMacro],
        _expected_revision: i64,
    ) -> Result<MathMacroSettings, StorageError> {
        unreachable!("note tests do not replace MathJax macros")
    }
}

/// 1件だけ登録された文献ライブラリ。所有者が一致する問い合わせにだけ答える。
pub(super) struct OneItemLibrary;

impl OneItemLibrary {
    pub(super) fn owner() -> PrincipalRef {
        principal("alice", 1)
    }

    fn item() -> BibliographyItem {
        BibliographyItem::create(
            marginalis_domain::BibliographyItemId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000021").expect("UUIDv7"),
            ),
            &Self::owner(),
            marginalis_domain::ValidatedCslJson::new(&serde_json::json!({
                "id": "smith2024",
                "type": "article-journal",
                "title": "An Example Article",
                "author": [{ "family": "Smith", "given": "Alex" }],
                "issued": { "date-parts": [[2024]] }
            }))
            .expect("valid CSL-JSON"),
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
            marginalis_domain::ValidatedCslJson::new(&serde_json::json!({
                "id": "tanaka2025",
                "type": "article-journal",
                "title": "追試の報告",
                "author": [{ "family": "Tanaka", "given": "Bun" }],
                "issued": { "date-parts": [[2025]] }
            }))
            .expect("valid CSL-JSON"),
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
        owner: &PrincipalRef,
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
        _csl_json: &marginalis_domain::ValidatedCslJson,
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
                position: crate::NoteSourcePosition { line: 1, column: 1 },
            }],
            attachment_queries: Vec::new(),
            citation_style: CitationStyle::default(),
            source_spans: Vec::new(),
        })
    }

    fn reference_queries(&self, _body: &str) -> Result<Vec<NoteReferenceQuery>, NoteContentError> {
        Ok(Vec::new())
    }

    fn citation_queries(&self, _body: &str) -> Result<Vec<NoteCitationQuery>, NoteContentError> {
        Ok(Vec::new())
    }

    fn attachment_queries(
        &self,
        _body: &str,
    ) -> Result<Vec<crate::NoteAttachmentQuery>, NoteContentError> {
        Ok(Vec::new())
    }

    fn citation_style(&self, _body: &str) -> Result<CitationStyle, NoteContentError> {
        Ok(CitationStyle::default())
    }

    fn outline(&self, _body: &str) -> Result<crate::NoteOutline, NoteContentError> {
        Ok(crate::NoteOutline {
            sections: Vec::new(),
            line_count: 0,
        })
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
