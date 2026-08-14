//! ノート操作の業務処理と、外側の実装に要求するport。

use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use marginalis_domain::{
    Actor, DeletedNoteListEntry, Note, NoteAccess, NoteAclEntry, NoteCreationSource, NoteDraft,
    NoteId, NoteListEntry, Revision,
};

mod access_control;
mod citations;
mod commands;
mod content;
mod graph;
mod patch;
mod presentation;
mod queries;
mod reviews;
mod sync;

pub use commands::NotePatchApplication;
pub use content::{
    NoteBibliographyEntry, NoteCitationQuery, NoteCitationResolution, NoteCitationSegment,
    NoteContent, NoteContentError, NoteLinkResolver, NoteOutline, NoteOutlineSection,
    NoteReferenceQuery, NoteReferenceResolution, NoteRenderInputs,
};
pub use graph::{
    NoteGraph, NoteGraphCitation, NoteGraphNote, NoteGraphQuery, NoteGraphReference, NoteGraphWork,
};
pub use patch::{NotePatchError, NotePatchOutcome, apply_note_patch};
pub use sync::{
    NOTE_SYNC_CURSOR_RETENTION_MS, NOTE_SYNC_DEFAULT_PAGE_SIZE, NOTE_SYNC_MAX_PAGE_SIZE,
    NoteSyncEntry, NoteSyncPage, NoteSyncPhase, NoteSyncRemovalReason,
};

use crate::{
    BibliographyRepository, Clock, MathMacroRepository, NoteAclChange, NoteAclState, NotePreview,
    NoteProfile, NoteRenderContext, NoteReviewDetails, NoteUseCaseError, NoteUseCases, NoteView,
    NoteWritePolicy, Random, RelatedNotes, StorageError,
};

/// 可視性を適用してノートを読み取るport。
#[async_trait]
pub trait NoteQueryRepository: Send + Sync {
    async fn list_visible_notes(
        &self,
        actor: &Actor,
        query: &crate::NoteListQuery,
    ) -> Result<Vec<NoteListEntry>, StorageError>;
    async fn list_owned_deleted_notes(
        &self,
        actor: &Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, StorageError>;
    async fn accessible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<AccessibleNote>, StorageError>;
    async fn visible_notes_by_id(
        &self,
        actor: &Actor,
        note_ids: &[NoteId],
    ) -> Result<Vec<Note>, StorageError>;
    async fn note_view_snapshot(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<NoteViewSnapshot>, StorageError>;
    /// 閲覧できるノートと、それらが引用する文献の関係を1回の読み取りで返す。
    async fn note_graph(
        &self,
        actor: &Actor,
        query: &NoteGraphQuery,
    ) -> Result<NoteGraph, StorageError>;
}

/// 現在の利用者が閲覧できるノートと、その利用者に対する実効アクセス水準。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessibleNote {
    pub note: Note,
    pub access: NoteAccess,
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
    async fn create_note(&self, note: &Note, links: NoteLinks<'_>) -> Result<(), StorageError>;
    #[allow(clippy::too_many_arguments)]
    async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        draft: &NoteDraft,
        links: NoteLinks<'_>,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, StorageError>;
    async fn soft_delete_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, StorageError>;
    async fn restore_owned_deleted_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, StorageError>;
}

/// 所有者だけが利用できるACL操作port。
#[async_trait]
pub trait NoteAclRepository: Send + Sync {
    async fn read_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, StorageError>;
    async fn replace_note_acl(
        &self,
        actor: &Actor,
        note_id: NoteId,
        entries: &[NoteAclEntry],
        expected_revision: Revision,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, StorageError>;
}

/// 所有者だけが利用できる人手確認操作port。
#[async_trait]
pub trait NoteReviewRepository: Send + Sync {
    async fn read_owned_note_review(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Note, StorageError>;
    async fn mark_owned_note_reviewed(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        reviewed_at: marginalis_domain::UnixMillis,
    ) -> Result<Note, StorageError>;
}

/// 検索用投影が、可視ノートの初期一覧と以後の変更を同じcursorで取得するport。
#[async_trait]
pub trait NoteSyncRepository: Send + Sync {
    async fn sync_notes(
        &self,
        actor: &Actor,
        cursor: Option<&str>,
        limit: usize,
        next_cursor: &str,
        now: marginalis_domain::UnixMillis,
    ) -> Result<NoteSyncPage, NoteSyncRepositoryError>;
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NoteSyncRepositoryError {
    #[error("sync cursor is invalid")]
    InvalidCursor,
    #[error("sync cursor has expired")]
    CursorExpired,
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// 本文から導いた、ノートが指し示す先の一覧。
///
/// ノート参照と引用は、どちらも本文の解析から得て同じtransactionで置き換える。別々のport
/// 引数にすると、片方だけ渡し忘れても型が通ってしまう。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoteLinks<'a> {
    pub reference_targets: &'a [NoteId],
    pub cited_keys: &'a [String],
}

/// `NoteApplication`が依存するportの束。
///
/// 同じ型のArcを位置引数で並べると、repositoryを取り違えても型検査で気づけない。
/// fieldの名前で結び付けを固定する。
pub struct NoteApplicationDependencies {
    pub queries: Arc<dyn NoteQueryRepository>,
    pub commands: Arc<dyn NoteCommandRepository>,
    pub access_control: Arc<dyn NoteAclRepository>,
    pub reviews: Arc<dyn NoteReviewRepository>,
    pub sync: Arc<dyn NoteSyncRepository>,
    pub content: Arc<dyn NoteContent>,
    pub bibliography: Arc<dyn BibliographyRepository>,
    pub math_macros: Arc<dyn MathMacroRepository>,
    pub links: Arc<dyn NoteLinkResolver>,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn Random>,
}

impl NoteApplicationDependencies {
    /// すべてのrepository portを同じstorage adapterが実装する構成をまとめて作る。
    ///
    /// composition rootがrepositoryごとに`Arc::new`を並べずに済む。
    pub fn with_storage<S>(
        storage: &Arc<S>,
        content: Arc<dyn NoteContent>,
        links: Arc<dyn NoteLinkResolver>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
    ) -> Self
    where
        S: NoteQueryRepository
            + NoteCommandRepository
            + NoteAclRepository
            + NoteReviewRepository
            + NoteSyncRepository
            + BibliographyRepository
            + MathMacroRepository
            + 'static,
    {
        Self {
            queries: storage.clone(),
            commands: storage.clone(),
            access_control: storage.clone(),
            reviews: storage.clone(),
            sync: storage.clone(),
            content,
            bibliography: storage.clone(),
            math_macros: storage.clone(),
            links,
            clock,
            random,
        }
    }
}

/// transportへ公開するノート操作のapplication service。
pub struct NoteApplication {
    queries: Arc<dyn NoteQueryRepository>,
    commands: Arc<dyn NoteCommandRepository>,
    access_control: Arc<dyn NoteAclRepository>,
    reviews: Arc<dyn NoteReviewRepository>,
    sync: Arc<dyn NoteSyncRepository>,
    content: Arc<dyn NoteContent>,
    bibliography: Arc<dyn BibliographyRepository>,
    math_macros: Arc<dyn MathMacroRepository>,
    links: Arc<dyn NoteLinkResolver>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
}

impl NoteApplication {
    pub fn new(dependencies: NoteApplicationDependencies) -> Self {
        Self {
            queries: dependencies.queries,
            commands: dependencies.commands,
            access_control: dependencies.access_control,
            reviews: dependencies.reviews,
            sync: dependencies.sync,
            content: dependencies.content,
            bibliography: dependencies.bibliography,
            math_macros: dependencies.math_macros,
            links: dependencies.links,
            clock: dependencies.clock,
            random: dependencies.random,
        }
    }

    async fn read_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Note, NoteUseCaseError> {
        self.queries
            .accessible_note(actor, note_id)
            .await
            .map_err(NoteUseCaseError::from)?
            .map(|accessible| accessible.note)
            .ok_or(NoteUseCaseError::NotFound)
    }
}

/// `NoteUseCases`の各操作は責務ごとのsubmoduleにある固有メソッドが実装し、ここでは
/// 呼び出しを委譲するだけにする。
#[async_trait]
impl NoteUseCases for NoteApplication {
    async fn list_visible_notes(
        &self,
        actor: Actor,
        query: crate::NoteListQuery,
    ) -> Result<Vec<NoteListEntry>, NoteUseCaseError> {
        NoteApplication::list_visible_notes(self, actor, query).await
    }

    async fn list_owned_deleted_notes(
        &self,
        actor: Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, NoteUseCaseError> {
        NoteApplication::list_owned_deleted_notes(self, actor).await
    }

    async fn read_note(&self, actor: Actor, note_id: NoteId) -> Result<Note, NoteUseCaseError> {
        NoteApplication::read_note(self, actor, note_id).await
    }

    async fn read_note_outline(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<(Note, NoteOutline), NoteUseCaseError> {
        NoteApplication::read_note_outline(self, actor, note_id).await
    }

    async fn read_note_fragment(
        &self,
        actor: Actor,
        note_id: NoteId,
        start_line: usize,
        end_line: usize,
    ) -> Result<(Note, String), NoteUseCaseError> {
        NoteApplication::read_note_fragment(self, actor, note_id, start_line, end_line).await
    }

    async fn apply_note_patch(
        &self,
        actor: Actor,
        note_id: NoteId,
        patch: &str,
        expected_revision: Revision,
        policy: NoteWritePolicy,
        dry_run: bool,
    ) -> Result<NotePatchApplication, NoteUseCaseError> {
        NoteApplication::apply_note_patch(
            self,
            actor,
            note_id,
            patch,
            expected_revision,
            policy,
            dry_run,
        )
        .await
    }

    async fn create_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        policy: NoteWritePolicy,
        created_via: NoteCreationSource,
    ) -> Result<Note, NoteUseCaseError> {
        NoteApplication::create_note(self, actor, draft, policy, created_via).await
    }

    async fn update_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        expected_revision: Revision,
        policy: NoteWritePolicy,
    ) -> Result<Note, NoteUseCaseError> {
        NoteApplication::update_note(self, actor, note_id, draft, expected_revision, policy).await
    }

    async fn soft_delete_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        NoteApplication::soft_delete_note(self, actor, note_id, expected_revision).await
    }

    async fn restore_note(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        NoteApplication::restore_note(self, actor, note_id, expected_revision).await
    }

    async fn preview_new_note(
        &self,
        actor: Actor,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        NoteApplication::preview_new_note(self, actor, draft, context).await
    }

    async fn preview_note_update(
        &self,
        actor: Actor,
        note_id: NoteId,
        draft: NoteDraft,
        context: NoteRenderContext,
    ) -> Result<NotePreview, NoteUseCaseError> {
        NoteApplication::preview_note_update(self, actor, note_id, draft, context).await
    }

    fn export_note_source(&self, note: &Note) -> Result<String, NoteUseCaseError> {
        NoteApplication::export_note_source(self, note)
    }

    async fn read_note_view(
        &self,
        actor: Actor,
        note_id: NoteId,
        context: NoteRenderContext,
    ) -> Result<NoteView, NoteUseCaseError> {
        NoteApplication::read_note_view(self, actor, note_id, context).await
    }

    async fn read_note_graph(
        &self,
        actor: Actor,
        query: NoteGraphQuery,
    ) -> Result<NoteGraph, NoteUseCaseError> {
        NoteApplication::read_note_graph(self, actor, query).await
    }

    fn note_profile(&self) -> NoteProfile {
        NoteApplication::note_profile(self)
    }

    async fn read_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteAclState, NoteUseCaseError> {
        NoteApplication::read_note_acl(self, actor, note_id).await
    }

    async fn replace_note_acl(
        &self,
        actor: Actor,
        note_id: NoteId,
        entries: Vec<NoteAclChange>,
        expected_revision: Revision,
    ) -> Result<Note, NoteUseCaseError> {
        NoteApplication::replace_note_acl(self, actor, note_id, entries, expected_revision).await
    }

    async fn read_note_review(
        &self,
        actor: Actor,
        note_id: NoteId,
    ) -> Result<NoteReviewDetails, NoteUseCaseError> {
        NoteApplication::read_note_review(self, actor, note_id).await
    }

    async fn mark_note_reviewed(
        &self,
        actor: Actor,
        note_id: NoteId,
        expected_revision: Revision,
    ) -> Result<NoteReviewDetails, NoteUseCaseError> {
        NoteApplication::mark_note_reviewed(self, actor, note_id, expected_revision).await
    }

    async fn sync_notes(
        &self,
        actor: Actor,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<NoteSyncPage, NoteUseCaseError> {
        NoteApplication::sync_notes(self, actor, cursor, limit).await
    }
}

/// 所有者の書誌ライブラリーやマクロ設定など、ノート本体ではない付随資源の読み取り失敗を写す。
///
/// `NotFound`や`Conflict`をそのまま返すと、ノート自体の不在や競合と区別できない。保存内容の
/// 破損以外は一時的な失敗として扱う。
fn map_owner_resource_error(error: StorageError) -> NoteUseCaseError {
    match error {
        StorageError::CorruptData => NoteUseCaseError::CorruptData,
        _ => NoteUseCaseError::Unavailable,
    }
}

/// 本文が名指したcitation keyを、重複なく並べる。
///
/// 書誌ライブラリーに実在するかどうかは問わない。ライブラリーは後から変わるため、保存する
/// のは「本文が何を引用したか」であって「解決できたか」ではない。
fn cited_keys(queries: &[NoteCitationQuery]) -> Vec<String> {
    let mut keys = queries
        .iter()
        .flat_map(|query| query.keys.iter().cloned())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}

fn reference_targets(queries: &[NoteReferenceQuery]) -> Vec<NoteId> {
    queries
        .iter()
        .map(|query| query.target_note_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod test_support;
