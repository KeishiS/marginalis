//! ノート操作の業務処理と、外側の実装に要求するport。

use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use marginalis_domain::{
    Actor, Note, NoteAclEntry, NoteCapabilities, NoteDraft, NoteId, NoteSummary, validate_identity,
};

use crate::{
    Clock, NoteProfile, NoteRenderContext, NoteUseCaseError, NoteUseCases, NoteValidationCode,
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

/// ノートaggregateを原子的に保存する永続化port。
///
/// 認可、期待revision、削除状態を伴う変更は、一つのmethod呼び出しを一つのtransactionとして
/// 実装する必要があります。
#[async_trait]
pub trait NoteRepository: Send + Sync {
    async fn list_visible_notes(&self, actor: &Actor) -> Result<Vec<Note>, NoteRepositoryError>;
    async fn visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<Note>, NoteRepositoryError>;
    async fn create_note(
        &self,
        note: &Note,
        reference_targets: &[NoteId],
    ) -> Result<(), NoteRepositoryError>;
    async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: i64,
        draft: &NoteDraft,
        reference_targets: &[NoteId],
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
    async fn soft_delete_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: i64,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
    async fn restore_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: i64,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
    async fn directly_related_notes(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<(Vec<NoteSummary>, Vec<NoteSummary>), NoteRepositoryError>;
    async fn note_capabilities(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteCapabilities>, NoteRepositoryError>;
    async fn read_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Vec<NoteAclEntry>, NoteRepositoryError>;
    async fn replace_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
        entries: &[NoteAclEntry],
        expected_revision: i64,
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
    repository: Arc<dyn NoteRepository>,
    content: Arc<dyn NoteContent>,
    links: Arc<dyn NoteLinkResolver>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
}

impl NoteApplication {
    pub fn new(
        repository: Arc<dyn NoteRepository>,
        content: Arc<dyn NoteContent>,
        links: Arc<dyn NoteLinkResolver>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
    ) -> Self {
        Self {
            repository,
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
        self.repository
            .visible_note(actor, note_id)
            .await
            .map_err(map_repository_error)?
            .ok_or(NoteUseCaseError::NotFound)
    }

    async fn reference_resolutions(
        &self,
        actor: &Actor,
        note: &Note,
        context: &NoteRenderContext,
    ) -> Result<Vec<NoteReferenceResolution>, NoteUseCaseError> {
        let queries = self
            .content
            .reference_queries(note.body())
            .map_err(|_| NoteUseCaseError::Unavailable)?;
        let mut resolutions = Vec::with_capacity(queries.len());
        for query in queries {
            let Some(target) = self
                .repository
                .visible_note(actor, query.target_note_id)
                .await
                .map_err(map_repository_error)?
            else {
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
impl NoteUseCases for NoteApplication {
    async fn list_visible_notes(&self, actor: Actor) -> Result<Vec<Note>, NoteUseCaseError> {
        self.repository
            .list_visible_notes(&actor)
            .await
            .map_err(map_repository_error)
    }

    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        self.read_visible_note(&actor, note_id).await
    }

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
        self.repository
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
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        let draft = self
            .content
            .validate_draft(draft)
            .map_err(NoteUseCaseError::Validation)?;
        self.read_visible_note(&actor, note_id).await?;
        let reference_targets = reference_targets(self.content.as_ref(), &draft.body)?;
        self.repository
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
        let resolutions = self.reference_resolutions(&actor, &note, &context).await?;
        self.content
            .render(&note, &resolutions)
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn soft_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        self.repository
            .soft_delete_visible_note(&actor, note_id, expected_revision, self.clock.now())
            .await
            .map_err(map_repository_error)
    }

    async fn restore_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        self.repository
            .restore_visible_note(&actor, note_id, expected_revision, self.clock.now())
            .await
            .map_err(map_repository_error)
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
        let note = self.read_visible_note(&actor, note_id).await?;
        let resolutions = self.reference_resolutions(&actor, &note, &context).await?;
        self.content
            .render(&note, &resolutions)
            .map_err(|_| NoteUseCaseError::Unavailable)
    }

    async fn related_notes(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<RelatedNotes, NoteUseCaseError> {
        self.read_visible_note(&actor, note_id).await?;
        let (mut outgoing, mut incoming) = self
            .repository
            .directly_related_notes(&actor, note_id)
            .await
            .map_err(map_repository_error)?;
        sort_related_notes(&mut outgoing);
        sort_related_notes(&mut incoming);
        Ok(RelatedNotes { outgoing, incoming })
    }

    async fn note_capabilities(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteCapabilities, NoteUseCaseError> {
        self.repository
            .note_capabilities(&actor, note_id)
            .await
            .map_err(map_repository_error)?
            .ok_or(NoteUseCaseError::NotFound)
    }

    async fn read_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<Vec<NoteAclEntry>, NoteUseCaseError> {
        self.repository
            .read_note_acl(&actor, note_id)
            .await
            .map_err(map_repository_error)
    }

    async fn replace_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
        mut entries: Vec<NoteAclEntry>,
        expected_revision: i64,
    ) -> Result<Note, NoteUseCaseError> {
        let note = self.read_visible_note(&actor, note_id).await?;
        entries.sort_by(|left, right| left.subject.cmp(&right.subject));
        for (index, entry) in entries.iter().enumerate() {
            validate_identity(note.creator_issuer(), &entry.subject)
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
        }
        self.repository
            .replace_note_acl(
                &actor,
                note_id,
                &entries,
                expected_revision,
                self.clock.now(),
            )
            .await
            .map_err(map_repository_error)
    }

    fn note_profile(&self) -> NoteProfile {
        self.content.profile()
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

    use marginalis_domain::{EntityId, UnixMillis};

    use super::*;

    #[derive(Default)]
    struct MemoryNotes {
        notes: Mutex<Vec<Note>>,
    }

    #[async_trait]
    impl NoteRepository for MemoryNotes {
        async fn list_visible_notes(
            &self,
            _actor: &Actor,
        ) -> Result<Vec<Note>, NoteRepositoryError> {
            Ok(self.notes.lock().expect("notes lock").clone())
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
            _expected_revision: i64,
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
            _expected_revision: i64,
            _now: UnixMillis,
        ) -> Result<Note, NoteRepositoryError> {
            Err(NoteRepositoryError::Unavailable)
        }

        async fn restore_visible_note(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
            _expected_revision: i64,
            _now: UnixMillis,
        ) -> Result<Note, NoteRepositoryError> {
            Err(NoteRepositoryError::Unavailable)
        }

        async fn directly_related_notes(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
        ) -> Result<(Vec<NoteSummary>, Vec<NoteSummary>), NoteRepositoryError> {
            Ok((Vec::new(), Vec::new()))
        }

        async fn note_capabilities(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
        ) -> Result<Option<NoteCapabilities>, NoteRepositoryError> {
            Ok(None)
        }

        async fn read_note_acl(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
        ) -> Result<Vec<NoteAclEntry>, NoteRepositoryError> {
            Ok(Vec::new())
        }

        async fn replace_note_acl(
            &self,
            _actor: &Actor,
            _note_id: NoteId,
            _entries: &[NoteAclEntry],
            _expected_revision: i64,
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
        assert_eq!(created.revision(), 1);
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
