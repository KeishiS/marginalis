//! ノート操作の業務処理と、外側の実装に要求するport。

use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use marginalis_domain::{
    Actor, DeletedNoteListEntry, Note, NoteAccess, NoteAclEntry, NoteDraft, NoteId, NoteListEntry,
    Revision,
};

mod access_control;
mod citations;
mod commands;
mod content;
mod graph;
mod presentation;
mod queries;

pub use content::{
    NoteBibliographyEntry, NoteCitationQuery, NoteCitationResolution, NoteCitationSegment,
    NoteContent, NoteContentError, NoteLinkResolver, NoteReferenceQuery, NoteReferenceResolution,
    NoteRenderInputs,
};
pub use graph::{
    NoteGraph, NoteGraphCitation, NoteGraphNote, NoteGraphQuery, NoteGraphReference, NoteGraphWork,
};

use crate::{
    BibliographyRepository, Clock, MathMacroRepository, NoteAclState, NoteUseCaseError, Random,
    RelatedNotes,
};

/// 永続化方式に依存しないrepositoryの失敗理由。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NoteRepositoryError {
    #[error("note was not found")]
    NotFound,
    #[error("note revision conflicts")]
    Conflict,
    #[error("note restoration period has expired")]
    RetentionExpired,
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
    async fn list_owned_deleted_notes(
        &self,
        actor: &Actor,
    ) -> Result<Vec<DeletedNoteListEntry>, NoteRepositoryError>;
    async fn accessible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
    ) -> Result<Option<AccessibleNote>, NoteRepositoryError>;
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
    /// 閲覧できるノートと、それらが引用する文献の関係を1回の読み取りで返す。
    async fn note_graph(
        &self,
        actor: &Actor,
        query: &NoteGraphQuery,
    ) -> Result<NoteGraph, NoteRepositoryError>;
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
    async fn create_note(
        &self,
        note: &Note,
        links: NoteLinks<'_>,
    ) -> Result<(), NoteRepositoryError>;
    #[allow(clippy::too_many_arguments)]
    async fn update_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        draft: &NoteDraft,
        links: NoteLinks<'_>,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
    async fn soft_delete_visible_note(
        &self,
        actor: &Actor,
        note_id: NoteId,
        expected_revision: Revision,
        now: marginalis_domain::UnixMillis,
    ) -> Result<Note, NoteRepositoryError>;
    async fn restore_owned_deleted_note(
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

/// 本文から導いた、ノートが指し示す先の一覧。
///
/// ノート参照と引用は、どちらも本文の解析から得て同じtransactionで置き換える。別々のport
/// 引数にすると、片方だけ渡し忘れても型が通ってしまう。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoteLinks<'a> {
    pub reference_targets: &'a [NoteId],
    pub cited_keys: &'a [String],
}

/// transportへ公開するノート操作のapplication service。
pub struct NoteApplication {
    queries: Arc<dyn NoteQueryRepository>,
    commands: Arc<dyn NoteCommandRepository>,
    access_control: Arc<dyn NoteAclRepository>,
    content: Arc<dyn NoteContent>,
    bibliography: Arc<dyn BibliographyRepository>,
    math_macros: Arc<dyn MathMacroRepository>,
    links: Arc<dyn NoteLinkResolver>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
}

impl NoteApplication {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        queries: Arc<dyn NoteQueryRepository>,
        commands: Arc<dyn NoteCommandRepository>,
        access_control: Arc<dyn NoteAclRepository>,
        content: Arc<dyn NoteContent>,
        bibliography: Arc<dyn BibliographyRepository>,
        math_macros: Arc<dyn MathMacroRepository>,
        links: Arc<dyn NoteLinkResolver>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
    ) -> Self {
        Self {
            queries,
            commands,
            access_control,
            content,
            bibliography,
            math_macros,
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
            .accessible_note(actor, note_id)
            .await
            .map_err(map_repository_error)?
            .map(|accessible| accessible.note)
            .ok_or(NoteUseCaseError::NotFound)
    }
}

fn map_repository_error(error: NoteRepositoryError) -> NoteUseCaseError {
    match error {
        NoteRepositoryError::NotFound => NoteUseCaseError::NotFound,
        NoteRepositoryError::Conflict => NoteUseCaseError::Conflict,
        NoteRepositoryError::RetentionExpired => NoteUseCaseError::RetentionExpired,
        NoteRepositoryError::CorruptData => NoteUseCaseError::CorruptData,
        NoteRepositoryError::Unavailable => NoteUseCaseError::Unavailable,
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
