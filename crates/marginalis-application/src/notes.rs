//! ノート操作の業務処理と、外側の実装に要求するport。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use marginalis_domain::{
    Actor, Identity, Note, NoteAccess, NoteAclEntry, NoteDraft, NoteId, NoteListEntry, NoteSummary,
    NoteValidationTarget, Revision,
};

use crate::{
    Clock, NoteAccessControl, NoteAclChange, NoteAclState, NoteCommands, NotePresentation,
    NotePreview, NoteProfile, NoteQueries, NoteRenderContext, NoteUseCaseError, NoteValidationCode,
    NoteValidationDiagnostic, NoteWritePolicy, Random, RelatedNotes, ValidatedNoteDraft,
};

/// 永続化方式に依存しないrepositoryの失敗理由。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NoteRepositoryError {
    #[error("note was not found")]
    NotFound,
    #[error("note revision conflicts")]
    Conflict,
    #[error("stored note data is invalid")]
    CorruptData,
    #[error("note storage is unavailable")]
    Unavailable,
}

/// 可視性を適用してノートを読み取るport。
#[async_trait]
pub trait NoteQueryRepository: Send + Sync {
    async fn list_visible_notes(
        &self,
        actor: &Actor,
    ) -> Result<Vec<NoteListEntry>, NoteRepositoryError>;
    async fn visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Note>, NoteRepositoryError>;
    async fn visible_notes_by_id(
        &self,
        actor: &Actor,
        note_ids: &[NoteId],
    ) -> Result<Vec<Note>, NoteRepositoryError>;
    async fn note_view_snapshot(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteViewSnapshot>, NoteRepositoryError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteViewSnapshot {
    pub note: Note,
    pub access: NoteAccess,
    pub reference_targets: Vec<Note>,
    pub related: RelatedNotes,
}

/// 認可、revision、削除状態を一つのtransactionへ拘束する変更port。
#[async_trait]
pub trait NoteCommandRepository: Send + Sync {
    async fn create_note(
        &self,
        note: &Note,
        reference_targets: &[NoteId],
    ) -> Result<(), NoteRepositoryError>;
    async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        draft: &NoteDraft,
        reference_targets: &[NoteId],
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
    async fn soft_delete_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
    async fn restore_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
}

/// 所有者だけが利用できるACL操作port。
#[async_trait]
pub trait NoteAclRepository: Send + Sync {
    async fn read_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, NoteRepositoryError>;
    async fn replace_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
        entries: &[NoteAclEntry],
        expected_revision: Revision,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
}

/// 文書内で見つかったノート参照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteReferenceQuery {
    pub reference_index: usize,
    pub target_note_id: NoteId,
    pub anchor: Option<String>,
}

/// 認可と外部URLの解決を終えたノート参照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteReferenceResolution {
    Visible {
        reference_index: usize,
        href: String,
        title: String,
        missing_anchor: bool,
    },
    Hidden {
        reference_index: usize,
    },
}

/// 文書adapterが保存済みの内容を解析または変換できない場合の失敗。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("note content could not be processed")]
pub struct NoteContentError;

/// AsciiDocなどの文書形式に依存する処理を受け持つport。
pub trait NoteContent: Send + Sync {
    fn validate_draft(
        &self,
        draft: NoteDraft,
    ) -> Result<ValidatedNoteDraft, Vec<NoteValidationDiagnostic>>;
    fn reference_queries(&self, body: &str) -> Result<Vec<NoteReferenceQuery>, NoteContentError>;
    fn has_anchor(&self, body: &str, anchor: &str) -> Result<bool, NoteContentError>;
    fn render(
        &self,
        note: &Note,
        resolutions: &[NoteReferenceResolution],
    ) -> Result<String, NoteContentError>;
    fn export(&self, note: &Note) -> Result<String, NoteContentError>;
    fn profile(&self) -> NoteProfile;
}

/// HTTPの配置方式に依存するノートURLを組み立てるport。
pub trait NoteLinkResolver: Send + Sync {
    fn href(
        &self,
        context: &NoteRenderContext,
        note_id: NoteId,
        anchor: Option<&str>,
    ) -> Option<String>;
}

/// transportへ公開するノート操作のapplication service。
pub struct NoteApplication {
    queries: Arc<dyn NoteQueryRepository>,
    commands: Arc<dyn NoteCommandRepository>,
    access_control: Arc<dyn NoteAclRepository>,
    content: Arc<dyn NoteContent>,
    links: Arc<dyn NoteLinkResolver>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
}

impl NoteApplication {
    pub fn new(
        queries: Arc<dyn NoteQueryRepository>,
        commands: Arc<dyn NoteCommandRepository>,
        access_control: Arc<dyn NoteAclRepository>,
        content: Arc<dyn NoteContent>,
        links: Arc<dyn NoteLinkResolver>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
    ) -> Self {
        Self {
            queries,
            commands,
            access_control,
            content,
            links,
            clock,
            random,
        }
    }

    async fn read_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Note, NoteUseCaseError> {
        self.queries
            .visible_note(actor, note_id)
            .await
            .map_err(map_repository_error)?
            .ok_or(NoteUseCaseError::NotFound)
    }

    fn reference_resolutions(
        &self,
        targets: &[Note],
        context: &NoteRenderContext,
        queries: &[NoteReferenceQuery],
    ) -> Result<Vec<NoteReferenceResolution>, NoteUseCaseError> {
        let targets = targets
            .iter()
            .cloned()
            .map(|note| (note.note_id(), note))
            .collect::<HashMap<_, _>>();
        let mut resolutions = Vec::with_capacity(queries.len());
        for query in queries {
            let Some(target) = targets.get(&query.target_note_id) else {
                resolutions.push(NoteReferenceResolution::Hidden {
                    reference_index: query.reference_index,
                });
                continue;
            };
            let missing_anchor = match query.anchor.as_deref() {
                Some(anchor) => !self
                    .content
                    .has_anchor(target.source(), anchor)
                    .map_err(|_| NoteUseCaseError::Unavailable)?,
                None => false,
            };
            let href = self
                .links
                .href(
                    context,
                    target.note_id(),
                    (!missing_anchor)
                        .then_some(query.anchor.as_deref())
                        .flatten(),
                )
                .ok_or(NoteUseCaseError::Unavailable)?;
            resolutions.push(NoteReferenceResolution::Visible {
                reference_index: query.reference_index,
                href,
                title: target.title().to_owned(),
                missing_anchor,
            });
        }
        Ok(resolutions)
    }
}

#[async_trait]
impl NoteQueries for NoteApplication {
    async fn list_visible_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        self.queries
            .list_visible_notes(&actor)
            .await
            .map_err(map_repository_error)
    }

    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.read_visible_note(&actor, note_id).await
    }
}

#[async_trait]
impl NoteCommands for NoteApplication {
    async fn create_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        let validated = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        let ValidatedNoteDraft {
            draft,
            diagnostics,
            reference_queries,
        } = validated;
        reject_warnings(policy, diagnostics)?;
        let now = self.clock.now();
        let note = Note::create(
            NoteId::new(self.random.uuid_v7()),
            actor.identity(),
            draft,
            now,
        );
        let reference_targets = reference_targets(&reference_queries);
        self.commands
            .create_note(&note, &reference_targets)
            .await
            .map_err(map_repository_error)?;
        Ok(note)
    }

    async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: Revision,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        let validated = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        let ValidatedNoteDraft {
            draft,
            diagnostics,
            reference_queries,
        } = validated;
        reject_warnings(policy, diagnostics)?;
        let reference_targets = reference_targets(&reference_queries);
        self.commands
            .update_visible_note(
                &actor,
                note_id,
                expected_revision,
                &draft,
                &reference_targets,
                self.clock.now(),
            )
            .await
            .map_err(map_repository_error)
    }

    async fn soft_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        self.commands
            .soft_delete_visible_note(&actor, note_id, expected_revision, self.clock.now())
            .await
            .map_err(map_repository_error)
    }

    async fn restore_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        self.commands
            .restore_visible_note(&actor, note_id, expected_revision, self.clock.now())
            .await
            .map_err(map_repository_error)
    }
}

fn reject_warnings(
    policy: NoteWritePolicy,
    diagnostics: Vec<crate::NoteAdvisoryDiagnostic>,
) -> Result<(), NoteUseCaseError> {
    if policy == NoteWritePolicy::RejectWarnings
        && diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::NoteAdvisorySeverity::Warning)
    {
        return Err(NoteUseCaseError::AdvisoriesRejected(diagnostics));
    }
    Ok(())
}

#[async_trait]
impl NotePresentation for NoteApplication {
    async fn preview_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        let validated = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        let ValidatedNoteDraft {
            draft,
            diagnostics,
            reference_queries,
        } = validated;
        let now = self.clock.now();
        let note = Note::create(
            NoteId::new(self.random.uuid_v7()),
            actor.identity(),
            draft,
            now,
        );
        let target_ids = reference_targets(&reference_queries);
        let targets = self
            .queries
            .visible_notes_by_id(&actor, &target_ids)
            .await
            .map_err(map_repository_error)?;
        let resolutions = self.reference_resolutions(&targets, &context, &reference_queries)?;
        let html = self
            .content
            .render(&note, &resolutions)
            .map_err(|_| NoteUseCaseError::RenderFailed)?;
        Ok(NotePreview { html, diagnostics })
    }

    fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        self.content
            .export(note)
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    fn note_profile(&self) -> NoteProfile {
        self.content.profile()
    }

    async fn read_note_view(
        &self,
        actor: Actor,
        note_id: NoteId,
        context: NoteRenderContext,
    ) -> Result<crate::NoteView, NoteUseCaseError> {
        let mut snapshot = self
            .queries
            .note_view_snapshot(&actor, note_id)
            .await
            .map_err(map_repository_error)?
            .ok_or(NoteUseCaseError::NotFound)?;
        sort_related_notes(&mut snapshot.related.outgoing);
        sort_related_notes(&mut snapshot.related.incoming);
        let reference_queries = self
            .content
            .reference_queries(snapshot.note.source())
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        let resolutions =
            self.reference_resolutions(&snapshot.reference_targets, &context, &reference_queries)?;
        let html = self
            .content
            .render(&snapshot.note, &resolutions)
            .map_err(|_| NoteUseCaseError::RenderFailed)?;
        Ok(crate::NoteView {
            note: snapshot.note,
            access: snapshot.access,
            html,
            related: snapshot.related,
        })
    }
}

#[async_trait]
impl NoteAccessControl for NoteApplication {
    async fn read_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, NoteUseCaseError> {
        self.access_control
            .read_note_acl(&actor, note_id)
            .await
            .map_err(map_repository_error)
    }

    async fn replace_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
        mut entries: Vec<NoteAclChange>,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        let note = self.read_visible_note(&actor, note_id).await?;
        entries.sort_by(|left, right| left.subject.cmp(&right.subject));
        let mut grants = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let identity =
                Identity::new(note.creator_issuer().to_owned(), entry.subject.clone())
                    .map_err(|_| acl_validation(index, AclValidationProblem::InvalidSubject))?;
            if entry.subject == note.creator_subject() {
                return Err(acl_validation(index, AclValidationProblem::OwnerIncluded));
            }
            if index > 0 && entries[index - 1].subject == entry.subject {
                return Err(acl_validation(
                    index,
                    AclValidationProblem::DuplicateSubject,
                ));
            }
            grants.push(NoteAclEntry::new(identity, entry.permission));
        }
        self.access_control
            .replace_note_acl(
                &actor,
                note_id,
                &grants,
                expected_revision,
                self.clock.now(),
            )
            .await
            .map_err(map_repository_error)
    }
}

fn map_repository_error(error: NoteRepositoryError) -> NoteUseCaseError {
    match error {
        NoteRepositoryError::NotFound => NoteUseCaseError::NotFound,
        NoteRepositoryError::Conflict => NoteUseCaseError::Conflict,
        NoteRepositoryError::CorruptData => NoteUseCaseError::CorruptData,
        NoteRepositoryError::Unavailable => NoteUseCaseError::Unavailable,
    }
}

/// ACL入力だけで起こる問題。
///
/// 全体の`NoteValidationCode`を受け取ると、ACLでは起こらない値も型の上では渡せてしまう。
/// 到達しないことをコメントで主張せず、渡せる値を型で限定する。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AclValidationProblem {
    InvalidSubject,
    DuplicateSubject,
    OwnerIncluded,
}

impl AclValidationProblem {
    const fn code(self) -> NoteValidationCode {
        match self {
            Self::InvalidSubject => NoteValidationCode::InvalidAclSubject,
            Self::DuplicateSubject => NoteValidationCode::DuplicateAclSubject,
            Self::OwnerIncluded => NoteValidationCode::OwnerInAcl,
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::InvalidSubject => "ACL subject is invalid",
            Self::DuplicateSubject => "ACL subject is duplicated",
            Self::OwnerIncluded => "note owner must not be included in ACL",
        }
    }
}

fn acl_validation(index: usize, problem: AclValidationProblem) -> NoteUseCaseError {
    NoteUseCaseError::Validation(vec![NoteValidationDiagnostic {
        code: problem.code().as_str().into(),
        target: NoteValidationTarget::AclEntry { index },
        span: None,
        message: problem.message().into(),
    }])
}

fn reference_targets(queries: &[NoteReferenceQuery]) -> Vec<NoteId> {
    queries
        .iter()
        .map(|query| query.target_note_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

fn sort_related_notes(notes: &mut [NoteSummary]) {
    notes.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.note_id.to_string().cmp(&right.note_id.to_string()))
    });
}

#[cfg(test)]
mod tests {
    use std::{
        str::FromStr,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use marginalis_domain::{EntityId, Revision, UnixMillis};

    use crate::{NoteAdvisoryDiagnostic, NoteAdvisorySeverity};

    use super::*;

    #[derive(Default)]
    struct MemoryNotes {
        notes: Mutex<Vec<Note>>,
        update_calls: AtomicUsize,
    }

    #[async_trait]
    impl NoteQueryRepository for MemoryNotes {
        async fn list_visible_notes(
            &self,
            _actor: &Actor,
        ) -> Result<Vec<NoteListEntry>, NoteRepositoryError> {
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

        async fn visible_note(
            &self,
            _actor: &Actor,
            note_id: NoteId,
        ) -> Result<Option<Note>, NoteRepositoryError> {
            Ok(self
                .notes
                .lock()
                .expect("notes lock")
                .iter()
                .find(|note| note.note_id() == note_id)
                .cloned())
        }

        async fn visible_notes_by_id(
            &self,
            actor: &Actor,
            note_ids: &[NoteId],
        ) -> Result<Vec<Note>, NoteRepositoryError> {
            let mut notes = Vec::new();
            for note_id in note_ids {
                if let Some(note) = self.visible_note(actor, *note_id).await? {
                    notes.push(note);
                }
            }
            Ok(notes)
        }

        async fn note_view_snapshot(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
        ) -> Result<Option<NoteViewSnapshot>, NoteRepositoryError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl NoteCommandRepository for MemoryNotes {
        async fn create_note(
            &self,
            note: &Note,
            _reference_targets: &[NoteId],
        ) -> Result<(), NoteRepositoryError> {
            self.notes.lock().expect("notes lock").push(note.clone());
            Ok(())
        }

        async fn update_visible_note(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
            _expected_revision: Revision,
            _draft: &NoteDraft,
            _reference_targets: &[NoteId],
            _now: UnixMillis,
        ) -> Result<Note, NoteRepositoryError> {
            self.update_calls.fetch_add(1, Ordering::Relaxed);
            Err(NoteRepositoryError::Unavailable)
        }

        async fn soft_delete_visible_note(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
            _expected_revision: Revision,
            _now: UnixMillis,
        ) -> Result<Note, NoteRepositoryError> {
            Err(NoteRepositoryError::Unavailable)
        }

        async fn restore_visible_note(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
            _expected_revision: Revision,
            _now: UnixMillis,
        ) -> Result<Note, NoteRepositoryError> {
            Err(NoteRepositoryError::Unavailable)
        }
    }

    #[async_trait]
    impl NoteAclRepository for MemoryNotes {
        async fn read_note_acl(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
        ) -> Result<NoteAclState, NoteRepositoryError> {
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
        ) -> Result<Note, NoteRepositoryError> {
            Err(NoteRepositoryError::Unavailable)
        }
    }

    #[derive(Default)]
    struct AcceptContent {
        reference_query_calls: AtomicUsize,
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
            })
        }

        fn reference_queries(
            &self,
            _body: &str,
        ) -> Result<Vec<NoteReferenceQuery>, NoteContentError> {
            self.reference_query_calls.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        }

        fn has_anchor(&self, _body: &str, _anchor: &str) -> Result<bool, NoteContentError> {
            Ok(false)
        }

        fn render(
            &self,
            _note: &Note,
            _resolutions: &[NoteReferenceResolution],
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

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> UnixMillis {
            UnixMillis::new(1_700_000_000_000)
        }
    }

    struct FixedRandom;

    impl Random for FixedRandom {
        fn uuid_v7(&self) -> EntityId {
            EntityId::from_str("01890f3c-6a4d-7cc2-98b3-84b68f68c6e1").expect("fixed UUIDv7")
        }

        fn opaque_token(&self) -> String {
            unreachable!("note creation does not issue an opaque token")
        }
    }

    struct NoLinks;

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

    #[tokio::test]
    async fn creates_a_note_using_only_application_ports() {
        let repository = Arc::new(MemoryNotes::default());
        let application = NoteApplication::new(
            repository.clone(),
            repository.clone(),
            repository.clone(),
            Arc::new(AcceptContent::default()),
            Arc::new(NoLinks),
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
        );
        let actor =
            Actor::try_new("https://id.example.test".into(), "alice".into()).expect("valid actor");

        let created = application
            .create_note(
                actor.clone(),
                NoteDraft {
                    source: "= Portで作成\n:tags: 設計\n\n本文".into(),
                    title: "Portで作成".into(),
                    tags: vec!["設計".into()],
                },
                NoteWritePolicy::AllowAdvisories,
            )
            .await
            .expect("create note");

        assert_eq!(created.creator_subject(), "alice");
        assert_eq!(created.revision().get(), 1);
        assert_eq!(
            application
                .read_note(actor, created.note_id())
                .await
                .expect("read created note"),
            created
        );
        assert_eq!(repository.notes.lock().expect("notes lock").len(), 1);
    }

    #[tokio::test]
    async fn preview_preserves_advisories_without_reanalyzing_or_blocking_save() {
        let repository = Arc::new(MemoryNotes::default());
        let content = Arc::new(AcceptContent::default());
        let application = NoteApplication::new(
            repository.clone(),
            repository.clone(),
            repository.clone(),
            content.clone(),
            Arc::new(NoLinks),
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
        );
        let actor =
            Actor::try_new("https://id.example.test".into(), "alice".into()).expect("valid actor");
        let draft = NoteDraft {
            source: "= Warning\n\nbody".into(),
            title: "Warning".into(),
            tags: Vec::new(),
        };

        let preview = application
            .preview_note(
                actor.clone(),
                draft.clone(),
                NoteRenderContext {
                    note_path_prefix: "/api/v3/notes".into(),
                },
            )
            .await
            .expect("warning does not reject preview");
        assert_eq!(preview.diagnostics.len(), 1);
        assert_eq!(preview.diagnostics[0].code, "test-advisory");
        assert_eq!(
            preview.diagnostics[0].severity,
            NoteAdvisorySeverity::Warning
        );
        assert_eq!(content.reference_query_calls.load(Ordering::Relaxed), 0);

        application
            .create_note(actor, draft, NoteWritePolicy::AllowAdvisories)
            .await
            .expect("warning does not reject save");
        assert_eq!(repository.notes.lock().expect("notes lock").len(), 1);
    }

    #[tokio::test]
    async fn strict_writes_reject_warnings_before_mutating_the_repository() {
        let repository = Arc::new(MemoryNotes::default());
        let application = NoteApplication::new(
            repository.clone(),
            repository.clone(),
            repository.clone(),
            Arc::new(AcceptContent::default()),
            Arc::new(NoLinks),
            Arc::new(FixedClock),
            Arc::new(FixedRandom),
        );
        let actor =
            Actor::try_new("https://id.example.test".into(), "alice".into()).expect("valid actor");
        let draft = NoteDraft {
            source: "= Warning\n\nbody".into(),
            title: "Warning".into(),
            tags: Vec::new(),
        };

        let create_error = application
            .create_note(
                actor.clone(),
                draft.clone(),
                NoteWritePolicy::RejectWarnings,
            )
            .await
            .expect_err("warning must reject strict create");
        let NoteUseCaseError::AdvisoriesRejected(diagnostics) = create_error else {
            panic!("strict create returned a different error");
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, NoteAdvisorySeverity::Warning);
        assert!(repository.notes.lock().expect("notes lock").is_empty());

        let existing = application
            .create_note(
                actor.clone(),
                draft.clone(),
                NoteWritePolicy::AllowAdvisories,
            )
            .await
            .expect("REST-compatible write accepts warnings");
        let update_error = application
            .update_note(
                actor.clone(),
                existing.note_id(),
                draft,
                existing.revision(),
                NoteWritePolicy::RejectWarnings,
            )
            .await
            .expect_err("warning must reject strict update");
        assert!(matches!(
            update_error,
            NoteUseCaseError::AdvisoriesRejected(_)
        ));
        assert_eq!(repository.update_calls.load(Ordering::Relaxed), 0);
        let unchanged = application
            .read_note(actor, existing.note_id())
            .await
            .expect("stored note remains readable");
        assert_eq!(unchanged.source(), existing.source());
        assert_eq!(unchanged.revision(), Revision::INITIAL);
    }

    #[test]
    fn strict_write_policy_allows_information_and_hints_without_a_warning() {
        let diagnostics = [
            NoteAdvisorySeverity::Information,
            NoteAdvisorySeverity::Hint,
        ]
        .into_iter()
        .map(|severity| NoteAdvisoryDiagnostic {
            code: "test-advisory".into(),
            severity,
            target: NoteValidationTarget::Source,
            span: None,
            message: "test advisory".into(),
        })
        .collect();

        assert_eq!(
            reject_warnings(NoteWritePolicy::RejectWarnings, diagnostics),
            Ok(())
        );
    }
}
