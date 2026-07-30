//! Marginalisの永続化方式から独立した業務モデルの実装。

use core::{fmt, str::FromStr};

use url::Url;
use uuid::Uuid;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidRevision;

impl fmt::Display for InvalidRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a revision must be a positive integer")
    }
}

impl std::error::Error for InvalidRevision {}

impl Revision {
    pub const INITIAL: Self = Self(1);

    pub const fn new(value: i64) -> Result<Self, InvalidRevision> {
        if value > 0 {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidEntityId;

impl fmt::Display for InvalidEntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an entity ID must be a UUIDv7")
    }
}

impl std::error::Error for InvalidEntityId {}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidBibliographyItem;

impl fmt::Display for InvalidBibliographyItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bibliography item metadata is inconsistent")
    }
}

impl std::error::Error for InvalidBibliographyItem {}

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidNote;

impl fmt::Display for InvalidNote {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("note metadata is inconsistent")
    }
}

impl std::error::Error for InvalidNote {}

impl Note {
    pub fn create(
        note_id: NoteId,
        owner: &Identity,
        draft: NoteDraft,
        created_at: UnixMillis,
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
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        note_id: NoteId,
        owner: Identity,
        title: String,
        source: String,
        tags: Vec<String>,
        created_at: UnixMillis,
        updated_at: UnixMillis,
        revision: Revision,
        deleted_at: Option<UnixMillis>,
    ) -> Result<Self, InvalidNote> {
        if created_at > updated_at
            || deleted_at
                .is_some_and(|deleted_at| deleted_at < created_at || deleted_at > updated_at)
        {
            return Err(InvalidNote);
        }
        Ok(Self {
            note_id,
            owner,
            title,
            source,
            tags,
            created_at,
            updated_at,
            revision,
            deleted_at,
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
}

/// 一覧表示に必要な、本文と所有者情報を含まないノート概要。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteSummary {
    pub note_id: NoteId,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at: UnixMillis,
    pub revision: Revision,
}

/// 一覧表示用の概要と、現在の利用者に対する実効アクセス水準。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteListEntry {
    pub summary: NoteSummary,
    pub access: NoteAccess,
}

impl From<&Note> for NoteSummary {
    fn from(note: &Note) -> Self {
        Self {
            note_id: note.note_id(),
            title: note.title().to_owned(),
            tags: note.tags().to_vec(),
            updated_at: note.updated_at(),
            revision: note.revision(),
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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

/// ノートに対する実効アクセス水準。大きい水準は小さい水準の操作を含む。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidIdentity;

impl fmt::Display for InvalidIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identity issuer or subject is invalid")
    }
}

impl std::error::Error for InvalidIdentity {}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAuthenticatedActor {
    pub actor: Actor,
    pub scopes: Vec<String>,
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
            Note::restore(
                note_id,
                owner.clone(),
                "Title".into(),
                "Body".into(),
                Vec::new(),
                UnixMillis::new(created_at),
                UnixMillis::new(updated_at),
                revision,
                deleted_at.map(UnixMillis::new),
            )
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
}
