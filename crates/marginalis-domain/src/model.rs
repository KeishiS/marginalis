//! Marginalisの永続化方式から独立した業務モデルの実装。

use core::{fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

/// 公開表現で受理するノートIDなど、永続的な識別子の文字列パターン。
///
/// REST・MCPのJSON Schemaはこの定数を参照し、実装が受理する規則と別に書かない。
pub const ENTITY_ID_PATTERN: &str =
    "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UnixMillis(i64);

impl UnixMillis {
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

/// 1から始まり、更新のたびに増えるノートの版番号。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Revision(i64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("a revision must be a positive integer")]
pub struct InvalidRevision;

impl Revision {
    /// 公開表現で受理する最小値。JSON Schemaの下限もこの値を参照する。
    pub const MINIMUM_VALUE: i64 = 1;

    pub const INITIAL: Self = Self(Self::MINIMUM_VALUE);

    pub const fn new(value: i64) -> Result<Self, InvalidRevision> {
        if value >= Self::MINIMUM_VALUE {
            Ok(Self(value))
        } else {
            Err(InvalidRevision)
        }
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EntityId(Uuid);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("an entity ID must be a UUIDv7")]
pub struct InvalidEntityId;

impl EntityId {
    pub fn try_from_uuid(value: Uuid) -> Result<Self, InvalidEntityId> {
        if value.get_version_num() == 7 {
            Ok(Self(value))
        } else {
            Err(InvalidEntityId)
        }
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl FromStr for EntityId {
    type Err = InvalidEntityId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value)
            .map_err(|_| InvalidEntityId)
            .and_then(Self::try_from_uuid)
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BibliographyItemId(EntityId);

impl BibliographyItemId {
    pub const fn new(value: EntityId) -> Self {
        Self(value)
    }

    pub const fn entity_id(self) -> EntityId {
        self.0
    }
}

impl fmt::Display for BibliographyItemId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyItem {
    item_id: BibliographyItemId,
    owner: Identity,
    citation_key: String,
    csl_json: String,
    created_at: UnixMillis,
    updated_at: UnixMillis,
    revision: Revision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("bibliography item metadata is inconsistent")]
pub struct InvalidBibliographyItem;

impl BibliographyItem {
    pub fn create(
        item_id: BibliographyItemId,
        owner: &Identity,
        citation_key: String,
        csl_json: String,
        created_at: UnixMillis,
    ) -> Self {
        Self {
            item_id,
            owner: owner.clone(),
            citation_key,
            csl_json,
            created_at,
            updated_at: created_at,
            revision: Revision::INITIAL,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        item_id: BibliographyItemId,
        owner: Identity,
        citation_key: String,
        csl_json: String,
        created_at: UnixMillis,
        updated_at: UnixMillis,
        revision: Revision,
    ) -> Result<Self, InvalidBibliographyItem> {
        if created_at > updated_at || citation_key.is_empty() || csl_json.is_empty() {
            return Err(InvalidBibliographyItem);
        }
        Ok(Self {
            item_id,
            owner,
            citation_key,
            csl_json,
            created_at,
            updated_at,
            revision,
        })
    }

    pub const fn item_id(&self) -> BibliographyItemId {
        self.item_id
    }

    pub const fn owner(&self) -> &Identity {
        &self.owner
    }

    pub fn citation_key(&self) -> &str {
        &self.citation_key
    }

    pub fn csl_json(&self) -> &str {
        &self.csl_json
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Note {
    note_id: NoteId,
    owner: Identity,
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
    reviewer: Identity,
}

impl NoteReviewRecord {
    pub const fn new(revision: Revision, reviewed_at: UnixMillis, reviewer: Identity) -> Self {
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

    pub const fn reviewer(&self) -> &Identity {
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
    pub owner: Identity,
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
        owner: &Identity,
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

    pub const fn owner(&self) -> &Identity {
        &self.owner
    }

    pub fn creator_issuer(&self) -> &str {
        self.owner.issuer()
    }

    pub fn creator_subject(&self) -> &str {
        self.owner.subject()
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
    identity: Identity,
    permission: NotePermission,
}

impl NoteAclEntry {
    pub const fn new(identity: Identity, permission: NotePermission) -> Self {
        Self {
            identity,
            permission,
        }
    }

    pub const fn identity(&self) -> &Identity {
        &self.identity
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
pub const MAX_IDENTITY_ISSUER_BYTES: usize = 2_048;
pub const MAX_IDENTITY_SUBJECT_BYTES: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("identity issuer or subject is invalid")]
pub struct InvalidIdentity;

pub fn validate_identity(issuer: &str, subject: &str) -> Result<(), InvalidIdentity> {
    let issuer_url = Url::parse(issuer).map_err(|_| InvalidIdentity)?;
    let issuer_valid = issuer.len() <= MAX_IDENTITY_ISSUER_BYTES
        && matches!(issuer_url.scheme(), "http" | "https")
        && !issuer_url.cannot_be_a_base()
        && issuer_url.username().is_empty()
        && issuer_url.password().is_none()
        && issuer_url.query().is_none()
        && issuer_url.fragment().is_none()
        && !issuer.chars().any(char::is_control);
    let subject_valid = !subject.is_empty()
        && subject.len() <= MAX_IDENTITY_SUBJECT_BYTES
        && !subject.chars().any(char::is_control);
    if issuer_valid && subject_valid {
        Ok(())
    } else {
        Err(InvalidIdentity)
    }
}

/// 外部identity providerが発行した、検証済みの主体識別子。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Identity {
    issuer: String,
    subject: String,
}

impl Identity {
    pub fn new(issuer: String, subject: String) -> Result<Self, InvalidIdentity> {
        validate_identity(&issuer, &subject)?;
        Ok(Self { issuer, subject })
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn into_parts(self) -> (String, String) {
        (self.issuer, self.subject)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Actor {
    identity: Identity,
}

impl Actor {
    pub const fn new(identity: Identity) -> Self {
        Self { identity }
    }

    pub fn try_new(issuer: String, subject: String) -> Result<Self, InvalidIdentity> {
        Ok(Self::new(Identity::new(issuer, subject)?))
    }

    pub const fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn issuer(&self) -> &str {
        self.identity.issuer()
    }

    pub fn subject(&self) -> &str {
        self.identity.subject()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSession {
    pub session_id: String,
    pub csrf_token: String,
    pub actor: Actor,
    pub idle_expires_at: UnixMillis,
    pub absolute_expires_at: UnixMillis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedSession {
    pub actor: Actor,
    pub idle_expires_at: UnixMillis,
    pub absolute_expires_at: UnixMillis,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_rejects_non_v7_uuid() {
        assert_eq!(EntityId::try_from_uuid(Uuid::nil()), Err(InvalidEntityId));
    }

    #[test]
    fn identity_rejects_active_content_and_unbounded_values() {
        assert_eq!(
            validate_identity("https://id.example.test", "alice"),
            Ok(())
        );
        assert_eq!(
            validate_identity("http://127.0.0.1:3000", "test-user"),
            Ok(())
        );
        for (issuer, subject) in [
            ("https://id.example.test\n:admin: true", "alice"),
            ("https://user@id.example.test", "alice"),
            ("https://id.example.test?tenant=other", "alice"),
            ("ftp://id.example.test", "alice"),
            ("https://id.example.test", "alice\n:admin: true"),
            ("https://id.example.test", ""),
        ] {
            assert_eq!(validate_identity(issuer, subject), Err(InvalidIdentity));
        }
        let long_issuer = format!(
            "https://id.example.test/{}",
            "a".repeat(MAX_IDENTITY_ISSUER_BYTES)
        );
        assert_eq!(
            validate_identity(&long_issuer, "alice"),
            Err(InvalidIdentity)
        );
        assert_eq!(
            validate_identity(
                "https://id.example.test",
                &"a".repeat(MAX_IDENTITY_SUBJECT_BYTES + 1)
            ),
            Err(InvalidIdentity)
        );
    }

    #[test]
    fn note_restoration_enforces_revision_and_time_ordering() {
        let note_id = NoteId::new(EntityId::try_from_uuid(Uuid::now_v7()).expect("UUIDv7"));
        let owner =
            Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner");
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
        let owner =
            Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner");
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
