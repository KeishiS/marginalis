//! ノート操作の業務処理と、外側の実装に要求するport。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use marginalis_domain::{
    Actor, Identity, Note, NoteAccess, NoteAclEntry, NoteDraft, NoteId, NoteSummary, Revision,
};

use crate::{
    Clock, NoteAccessControl, NoteAclChange, NoteAclState, NoteCommands, NotePresentation,
    NoteProfile, NoteQueries, NoteRenderContext, NoteUseCaseError, NoteValidationCode,
    NoteValidationDiagnostic, NoteValidationTarget, Random, RelatedNotes,
};

/// 永続化方式に依存しないrepositoryの失敗理由。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteRepositoryError {
    NotFound,
    Conflict,
    CorruptData,
    Unavailable,
}

/// 可視性を適用してノートを読み取るport。
#[async_trait]
pub trait NoteQueryRepository: Send + Sync {
    async fn list_visible_notes(
        &self,
        actor: &Actor,
    ) -> Result<Vec<NoteSummary>, NoteRepositoryError>;
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
    async fn directly_related_notes(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<(Vec<NoteSummary>, Vec<NoteSummary>), NoteRepositoryError>;
    async fn note_access(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteAccess>, NoteRepositoryError>;
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoteContentError;

impl std::fmt::Display for NoteContentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("note content could not be processed")
    }
}

impl std::error::Error for NoteContentError {}

/// AsciiDocなどの文書形式に依存する処理を受け持つport。
pub trait NoteContent: Send + Sync {
    fn validate_draft(&self, draft: NoteDraft) -> Result<NoteDraft, Vec<NoteValidationDiagnostic>>;
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
        note: &Note,
        targets: &[Note],
        context: &NoteRenderContext,
    ) -> Result<Vec<NoteReferenceResolution>, NoteUseCaseError> {
        let queries = self
            .content
            .reference_queries(note.body())
            .map_err(|_| NoteUseCaseError::Unavailable)?;
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
                    .has_anchor(target.body(), anchor)
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
    async fn list_visible_notes(&self, actor: Actor) -> Result<Vec<NoteSummary>, NoteUseCaseError> {
        self.queries
            .list_visible_notes(&actor)
            .await
            .map_err(map_repository_error)
    }

    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.read_visible_note(&actor, note_id).await
    }

    async fn related_notes(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<RelatedNotes, NoteUseCaseError> {
        self.read_visible_note(&actor, note_id).await?;
        let (mut outgoing, mut incoming) = self
            .queries
            .directly_related_notes(&actor, note_id)
            .await
            .map_err(map_repository_error)?;
        sort_related_notes(&mut outgoing);
        sort_related_notes(&mut incoming);
        Ok(RelatedNotes { outgoing, incoming })
    }

    async fn note_access(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteAccess, NoteUseCaseError> {
        self.queries
            .note_access(&actor, note_id)
            .await
            .map_err(map_repository_error)?
            .ok_or(NoteUseCaseError::NotFound)
    }
}

#[async_trait]
impl NoteCommands for NoteApplication {
    async fn create_note(&self, actor: Actor, draft: NoteDraft) -> Result<Note, NoteUseCaseError> {
        let draft = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        let now = self.clock.now();
        let note = Note::create(
            NoteId::new(self.random.uuid_v7()),
            actor.identity(),
            draft,
            now,
        );
        let reference_targets = reference_targets(self.content.as_ref(), note.body())?;
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
    ) -> Result<Note, NoteUseCaseError> {
        let draft = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        self.read_visible_note(&actor, note_id).await?;
        let reference_targets = reference_targets(self.content.as_ref(), &draft.body)?;
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

#[async_trait]
impl NotePresentation for NoteApplication {
    async fn preview_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<String, NoteUseCaseError> {
        let draft = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        let now = self.clock.now();
        let note = Note::create(
            NoteId::new(self.random.uuid_v7()),
            actor.identity(),
            draft,
            now,
        );
        let target_ids = reference_targets(self.content.as_ref(), note.body())?;
        let targets = self
            .queries
            .visible_notes_by_id(&actor, &target_ids)
            .await
            .map_err(map_repository_error)?;
        let resolutions = self.reference_resolutions(&note, &targets, &context)?;
        self.content
            .render(&note, &resolutions)
            .map_err(|_| NoteUseCaseError::RenderFailed)
    }

    fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        self.content
            .export(note)
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn render_note_html(
        &self,
        actor: Actor,
        note_id: NoteId,
        context: NoteRenderContext,
    ) -> Result<String, NoteUseCaseError> {
        Ok(self.read_note_view(actor, note_id, context).await?.html)
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
        let resolutions =
            self.reference_resolutions(&snapshot.note, &snapshot.reference_targets, &context)?;
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
                    .map_err(|_| acl_validation(index, NoteValidationCode::InvalidAclSubject))?;
            if entry.subject == note.creator_subject() {
                return Err(acl_validation(index, NoteValidationCode::OwnerInAcl));
            }
            if index > 0 && entries[index - 1].subject == entry.subject {
                return Err(acl_validation(
                    index,
                    NoteValidationCode::DuplicateAclSubject,
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
        NoteRepositoryError::CorruptData | NoteRepositoryError::Unavailable => {
            NoteUseCaseError::Unavailable
        }
    }
}

fn acl_validation(index: usize, code: NoteValidationCode) -> NoteUseCaseError {
    let message = match code {
        NoteValidationCode::InvalidAclSubject => "ACL subject is invalid",
        NoteValidationCode::DuplicateAclSubject => "ACL subject is duplicated",
        NoteValidationCode::OwnerInAcl => "note owner must not be included in ACL",
        _ => unreachable!("ACL validation uses an ACL-specific code"),
    };
    NoteUseCaseError::Validation(vec![NoteValidationDiagnostic {
        code,
        target: NoteValidationTarget::AclEntry { index },
        span: None,
        message,
    }])
}

fn reference_targets(
    content: &dyn NoteContent,
    body: &str,
) -> Result<Vec<NoteId>, NoteUseCaseError> {
    Ok(content
        .reference_queries(body)
        .map_err(|_| NoteUseCaseError::Unavailable)?
        .into_iter()
        .map(|query| query.target_note_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect())
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
    use std::{str::FromStr, sync::Mutex};

    use marginalis_domain::{EntityId, Revision, UnixMillis};

    use super::*;

    #[derive(Default)]
    struct MemoryNotes {
        notes: Mutex<Vec<Note>>,
    }

    #[async_trait]
    impl NoteQueryRepository for MemoryNotes {
        async fn list_visible_notes(
            &self,
            _actor: &Actor,
        ) -> Result<Vec<NoteSummary>, NoteRepositoryError> {
            Ok(self
                .notes
                .lock()
                .expect("notes lock")
                .iter()
                .map(NoteSummary::from)
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

        async fn directly_related_notes(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
        ) -> Result<(Vec<NoteSummary>, Vec<NoteSummary>), NoteRepositoryError> {
            Ok((Vec::new(), Vec::new()))
        }

        async fn note_access(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
        ) -> Result<Option<NoteAccess>, NoteRepositoryError> {
            Ok(None)
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

    struct AcceptContent;

    impl NoteContent for AcceptContent {
        fn validate_draft(
            &self,
            draft: NoteDraft,
        ) -> Result<NoteDraft, Vec<NoteValidationDiagnostic>> {
            Ok(draft)
        }

        fn reference_queries(
            &self,
            _body: &str,
        ) -> Result<Vec<NoteReferenceQuery>, NoteContentError> {
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
            Ok(String::new())
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
            Arc::new(AcceptContent),
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
                    title: "Portで作成".into(),
                    body: "本文".into(),
                    tags: vec!["設計".into()],
                },
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
}
