//! Marginalisの永続化方式から独立した業務モデル。

use core::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// UTC epoch millisecondsで表すアプリケーション時刻。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct UnixMillis(i64);

impl UnixMillis {
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

/// AsciiDoc正本から算出するSHA-256 revision。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceRevision([u8; 32]);

impl SourceRevision {
    pub fn from_source(source: &[u8]) -> Self {
        Self(Sha256::digest(source).into())
    }

    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn from_bytes(value: &[u8]) -> Option<Self> {
        value.try_into().ok().map(Self)
    }

    /// HTTP ETagおよび監査ログに使う、固定長の小文字16進表現。
    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn from_hex(value: &str) -> Option<Self> {
        if value.len() != 64 {
            return None;
        }
        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }
}

/// Marginalisが生成したUUIDv7だけを受け入れるID。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
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

    pub const fn from_uuid_v7(value: Uuid) -> Self {
        Self(value)
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct UserId(EntityId);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct NoteId(EntityId);

macro_rules! entity_id {
    ($name:ident) => {
        impl $name {
            pub const fn new(value: EntityId) -> Self {
                Self(value)
            }

            pub const fn entity_id(self) -> EntityId {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

entity_id!(UserId);
entity_id!(NoteId);

/// 認可済みのノート正本と、その内容に一意に対応するrevision。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteSource {
    pub note_id: NoteId,
    pub title: String,
    pub content: Vec<u8>,
    pub revision: SourceRevision,
}

/// SQLite検索・参照解決に使う、ノート正本から抽出済みの投影。
///
/// `title`、anchorおよび参照はAsciiDoc adapterが検証してから渡す。domainは構文木を持たない。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteProjection {
    pub note_id: NoteId,
    pub owner_id: UserId,
    pub title: String,
    /// 検索専用の正規化前テキスト。正本更新と同じtransactionで投影へ反映する。
    pub search_text: String,
    pub anchors: Vec<String>,
    pub references: Vec<NoteReference>,
}

/// ACLで可視なノートの一覧・検索に共通するread model。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteSummary {
    pub note_id: NoteId,
    pub title: String,
}

/// ACL適用後の一覧・検索結果の一頁。offsetはtransportが不透明cursorへ符号化する。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotePage {
    pub notes: Vec<NoteSummary>,
    pub next_offset: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteReference {
    pub source_start: u32,
    pub source_end: u32,
    pub target_note_id: String,
    pub target_anchor: Option<String>,
}

/// 認証済み主体。rootは通常ユーザーのACLを管理できる。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Actor {
    pub user_id: UserId,
    pub is_root: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RegistrationPolicy {
    Open,
    #[default]
    Approval,
    InviteOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserStatus {
    Pending,
    Active,
    Disabled,
}

impl UserStatus {
    pub const fn as_storage(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// OIDCの検証済みID tokenから抽出した、本人同定と表示のための情報。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcIdentity {
    pub issuer: String,
    pub subject: String,
    pub display_name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidOidcIdentity;

impl fmt::Display for InvalidOidcIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OIDC issuer and subject must not be empty")
    }
}

impl std::error::Error for InvalidOidcIdentity {}

impl OidcIdentity {
    pub fn new(
        issuer: impl Into<String>,
        subject: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Result<Self, InvalidOidcIdentity> {
        let identity = Self {
            issuer: issuer.into(),
            subject: subject.into(),
            display_name: display_name.into(),
        };
        if identity.issuer.trim().is_empty() || identity.subject.trim().is_empty() {
            Err(InvalidOidcIdentity)
        } else {
            Ok(identity)
        }
    }
}

/// OIDC identityに紐付く内部ユーザー。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OidcUser {
    pub user_id: UserId,
    pub status: UserStatus,
    pub display_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OidcLoginResult {
    Active(OidcUser),
    PendingApproval(OidcUser),
    RegistrationDenied,
    Disabled(OidcUser),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NotePermission {
    Read,
    Write,
    Admin,
}

impl NotePermission {
    pub fn permits(self, required: Self) -> bool {
        self >= required
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_rejects_non_v7_uuid() {
        assert_eq!(EntityId::try_from_uuid(Uuid::nil()), Err(InvalidEntityId));
    }

    #[test]
    fn permissions_are_ordered() {
        assert!(NotePermission::Admin.permits(NotePermission::Write));
        assert!(!NotePermission::Read.permits(NotePermission::Write));
    }

    #[test]
    fn source_revision_round_trips_through_hex() {
        let revision = SourceRevision::from_source(b"source");
        assert_eq!(SourceRevision::from_hex(&revision.to_hex()), Some(revision));
        assert_eq!(SourceRevision::from_hex("not-a-revision"), None);
    }
}
