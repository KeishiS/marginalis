//! Marginalisの永続化方式から独立した業務モデル。

use core::{fmt, str::FromStr};

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// UTC epoch millisecondsで表すアプリケーション時刻。
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
}

/// Marginalisが生成したUUIDv7だけを受け入れるID。
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UserId(EntityId);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
}
