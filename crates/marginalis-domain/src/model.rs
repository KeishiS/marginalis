//! Marginalisの永続化方式から独立した業務モデルの実装。
//!
//! 共通の識別子と版番号をこのファイルで定義し、ノート、文献、identityの各系統は
//! 子moduleへ分ける。公開名はすべて`marginalis_domain`直下から変わらない。

mod attachment;
mod bibliography;
mod identity;
mod note;

pub use attachment::*;
pub use bibliography::*;
pub use identity::*;
pub use note::*;

use core::{fmt, str::FromStr};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_id_rejects_non_v7_uuid() {
        assert_eq!(EntityId::try_from_uuid(Uuid::nil()), Err(InvalidEntityId));
    }
}
