//! ノートの正本、確認状態、ACL、アクセス水準。

use core::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{EntityId, PrincipalRef, Revision, UnixMillis};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NoteId(EntityId);

impl NoteId {
    pub const fn new(value: EntityId) -> Self {
        Self(value)
    }

    pub const fn entity_id(self) -> EntityId {
        self.0
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Note {
    note_id: NoteId,
    owner: PrincipalRef,
    title: String,
    source: String,
    tags: Vec<String>,
    created_at: UnixMillis,
    updated_at: UnixMillis,
    revision: Revision,
    deleted_at: Option<UnixMillis>,
    created_via: NoteCreationSource,
    review: NoteReviewTracking,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("note metadata is inconsistent")]
pub struct InvalidNote;

/// ノートを最初に保存した、サーバー側で判定する接続経路。
///
/// 作成者の種類、AIの利用、内容の品質を証明する値ではない。
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteCreationSource {
    Web,
    Rest,
    Mcp,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("note creation source is invalid")]
pub struct InvalidNoteCreationSource;

impl NoteCreationSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Rest => "rest",
            Self::Mcp => "mcp",
            Self::Unknown => "unknown",
        }
    }
}

impl FromStr for NoteCreationSource {
    type Err = InvalidNoteCreationSource;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "web" => Ok(Self::Web),
            "rest" => Ok(Self::Rest),
            "mcp" => Ok(Self::Mcp),
            "unknown" => Ok(Self::Unknown),
            _ => Err(InvalidNoteCreationSource),
        }
    }
}

/// 現在のrevisionに対する人手確認状態。
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteReviewStatus {
    Unknown,
    Pending,
    Reviewed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("note review status is invalid")]
pub struct InvalidNoteReviewStatus;

impl NoteReviewStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Pending => "pending",
            Self::Reviewed => "reviewed",
        }
    }
}

impl FromStr for NoteReviewStatus {
    type Err = InvalidNoteReviewStatus;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "pending" => Ok(Self::Pending),
            "reviewed" => Ok(Self::Reviewed),
            _ => Err(InvalidNoteReviewStatus),
        }
    }
}

/// 所有者が明示的に確認したノートのrevisionと確認者。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteReviewRecord {
    revision: Revision,
    reviewed_at: UnixMillis,
    reviewer: PrincipalRef,
}

impl NoteReviewRecord {
    pub const fn new(revision: Revision, reviewed_at: UnixMillis, reviewer: PrincipalRef) -> Self {
        Self {
            revision,
            reviewed_at,
            reviewer,
        }
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub const fn reviewed_at(&self) -> UnixMillis {
        self.reviewed_at
    }

    pub const fn reviewer(&self) -> &PrincipalRef {
        &self.reviewer
    }
}

/// 旧形式の情報不足と、確認の有無を追跡している状態を区別する保存値。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoteReviewTracking {
    Unknown,
    Tracked {
        last_review: Option<NoteReviewRecord>,
    },
}

impl NoteReviewTracking {
    pub const fn pending() -> Self {
        Self::Tracked { last_review: None }
    }

    pub const fn tracked(last_review: Option<NoteReviewRecord>) -> Self {
        Self::Tracked { last_review }
    }

    pub fn status(&self, current_revision: Revision) -> NoteReviewStatus {
        match self {
            Self::Unknown => NoteReviewStatus::Unknown,
            Self::Tracked {
                last_review: Some(review),
            } if review.revision == current_revision => NoteReviewStatus::Reviewed,
            Self::Tracked { .. } => NoteReviewStatus::Pending,
        }
    }

    pub const fn last_review(&self) -> Option<&NoteReviewRecord> {
        match self {
            Self::Unknown | Self::Tracked { last_review: None } => None,
            Self::Tracked {
                last_review: Some(review),
            } => Some(review),
        }
    }
}

/// 保存済みノートを復元するための、保存方式に依存しない値一式。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteRestore {
    pub note_id: NoteId,
    pub owner: PrincipalRef,
    pub draft: NoteDraft,
    pub created_at: UnixMillis,
    pub updated_at: UnixMillis,
    pub revision: Revision,
    pub deleted_at: Option<UnixMillis>,
    pub created_via: NoteCreationSource,
    pub review: NoteReviewTracking,
}

impl Note {
    pub fn create(
        note_id: NoteId,
        owner: &PrincipalRef,
        draft: NoteDraft,
        created_at: UnixMillis,
        created_via: NoteCreationSource,
    ) -> Self {
        Self {
            note_id,
            owner: owner.clone(),
            title: draft.title,
            source: draft.source,
            tags: draft.tags,
            created_at,
            updated_at: created_at,
            revision: Revision::INITIAL,
            deleted_at: None,
            created_via,
            review: NoteReviewTracking::pending(),
        }
    }

    pub fn restore(restored: NoteRestore) -> Result<Self, InvalidNote> {
        let NoteRestore {
            note_id,
            owner,
            draft,
            created_at,
            updated_at,
            revision,
            deleted_at,
            created_via,
            review,
        } = restored;
        if created_at > updated_at
            || deleted_at
                .is_some_and(|deleted_at| deleted_at < created_at || deleted_at > updated_at)
            || review.last_review().is_some_and(|last_review| {
                last_review.revision > revision
                    || last_review.reviewed_at < created_at
                    || last_review.reviewed_at > updated_at
                    || last_review.reviewer != owner
            })
        {
            return Err(InvalidNote);
        }
        Ok(Self {
            note_id,
            owner,
            title: draft.title,
            source: draft.source,
            tags: draft.tags,
            created_at,
            updated_at,
            revision,
            deleted_at,
            created_via,
            review,
        })
    }

    pub const fn note_id(&self) -> NoteId {
        self.note_id
    }

    pub const fn owner(&self) -> &PrincipalRef {
        &self.owner
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub const fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    pub const fn updated_at(&self) -> UnixMillis {
        self.updated_at
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub const fn deleted_at(&self) -> Option<UnixMillis> {
        self.deleted_at
    }

    pub const fn created_via(&self) -> NoteCreationSource {
        self.created_via
    }

    pub fn review_status(&self) -> NoteReviewStatus {
        self.review.status(self.revision)
    }

    pub const fn last_review(&self) -> Option<&NoteReviewRecord> {
        self.review.last_review()
    }

    pub const fn review_tracking_known(&self) -> bool {
        matches!(self.review, NoteReviewTracking::Tracked { .. })
    }
}

/// 一覧表示に必要な、本文と所有者情報を含まないノート概要。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteSummary {
    pub note_id: NoteId,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at: UnixMillis,
    pub revision: Revision,
    pub created_via: NoteCreationSource,
    pub review_status: NoteReviewStatus,
    pub reviewed_revision: Option<Revision>,
    pub reviewed_at: Option<UnixMillis>,
}

/// 一覧表示用の概要と、現在の利用者に対する実効アクセス水準。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteListEntry {
    pub summary: NoteSummary,
    pub access: NoteAccess,
}

/// 所有者向けの削除済み一覧に必要な、本文と共有先を含まないノート概要。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeletedNoteListEntry {
    pub note_id: NoteId,
    pub title: String,
    pub deleted_at: UnixMillis,
    pub purge_at: UnixMillis,
    pub revision: Revision,
}

impl From<&Note> for NoteSummary {
    fn from(note: &Note) -> Self {
        Self {
            note_id: note.note_id(),
            title: note.title().to_owned(),
            tags: note.tags().to_vec(),
            updated_at: note.updated_at(),
            revision: note.revision(),
            created_via: note.created_via(),
            review_status: note.review_status(),
            reviewed_revision: note.last_review().map(NoteReviewRecord::revision),
            reviewed_at: note.last_review().map(NoteReviewRecord::reviewed_at),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteDraft {
    /// 利用者が記述した完全なAsciiDoc文書。
    pub source: String,
    /// `source`の文書題名から検証時に導出した値。
    pub title: String,
    /// `source`の`tags`属性から検証時に導出した値。
    pub tags: Vec<String>,
}

/// ACLで共有先へ与える権限。REST、MCP、archiveで同じ表現を使用する。
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NotePermission {
    Read,
    Edit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteAclEntry {
    principal: PrincipalRef,
    permission: NotePermission,
}

impl NoteAclEntry {
    pub const fn new(principal: PrincipalRef, permission: NotePermission) -> Self {
        Self {
            principal,
            permission,
        }
    }

    pub const fn principal(&self) -> &PrincipalRef {
        &self.principal
    }

    pub const fn permission(&self) -> NotePermission {
        self.permission
    }
}

/// 入力上の問題が、ノート入力のどの部分にあるかを示す位置。
///
/// REST、MCP、Web UIで同じ表現を使用する。`field`を判別子とし、`tag`と`acl_entry`は
/// 対象の添字を伴う。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "field", rename_all = "snake_case", deny_unknown_fields)]
pub enum NoteValidationTarget {
    Source,
    Title,
    Body,
    Tag { index: usize },
    Tags,
    AclEntry { index: usize },
}

/// 入力上の問題が置かれた、UTF-8で符号化した`source`上のバイト範囲。
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Utf8ByteSpan {
    pub start: u32,
    pub end: u32,
}

/// ノートに対する実効アクセス水準。大きい水準は小さい水準の操作を含む。
///
/// REST、MCP、Web UIで同じ表現を使用する。
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NoteAccess {
    Read,
    Edit,
    Manage,
}

impl NoteAccess {
    pub const fn allows(self, required: Self) -> bool {
        self as u8 >= required as u8
    }
}

pub const SOFT_DELETE_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::model::{Identity, InvalidRevision, PrincipalId};

    fn owner() -> PrincipalRef {
        PrincipalRef::new(
            PrincipalId::new(1).expect("principal ID"),
            Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
        )
    }

    #[test]
    fn note_restoration_enforces_revision_and_time_ordering() {
        let note_id = NoteId::new(EntityId::try_from_uuid(Uuid::now_v7()).expect("UUIDv7"));
        let owner = owner();
        let restore = |created_at, updated_at, revision, deleted_at: Option<i64>| {
            Note::restore(NoteRestore {
                note_id,
                owner: owner.clone(),
                draft: NoteDraft {
                    title: "Title".into(),
                    source: "Body".into(),
                    tags: Vec::new(),
                },
                created_at: UnixMillis::new(created_at),
                updated_at: UnixMillis::new(updated_at),
                revision,
                deleted_at: deleted_at.map(UnixMillis::new),
                created_via: NoteCreationSource::Unknown,
                review: NoteReviewTracking::Unknown,
            })
        };

        assert_eq!(Revision::new(0), Err(InvalidRevision));
        assert!(restore(100, 100, Revision::INITIAL, None).is_ok());
        assert_eq!(restore(101, 100, Revision::INITIAL, None), Err(InvalidNote));
        assert_eq!(
            restore(100, 200, Revision::INITIAL, Some(99)),
            Err(InvalidNote)
        );
        assert_eq!(
            restore(100, 200, Revision::INITIAL, Some(201)),
            Err(InvalidNote)
        );
    }

    #[test]
    fn review_status_distinguishes_unknown_pending_current_and_stale() {
        let owner = owner();
        let current = Revision::new(3).expect("revision");

        assert_eq!(
            NoteReviewTracking::Unknown.status(current),
            NoteReviewStatus::Unknown
        );
        assert_eq!(
            NoteReviewTracking::pending().status(current),
            NoteReviewStatus::Pending
        );
        assert_eq!(
            NoteReviewTracking::tracked(Some(NoteReviewRecord::new(
                current,
                UnixMillis::new(200),
                owner.clone(),
            )))
            .status(current),
            NoteReviewStatus::Reviewed
        );
        assert_eq!(
            NoteReviewTracking::tracked(Some(NoteReviewRecord::new(
                Revision::new(2).expect("revision"),
                UnixMillis::new(150),
                owner,
            )))
            .status(current),
            NoteReviewStatus::Pending
        );
    }
}
