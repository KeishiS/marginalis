//! Marginalisの永続化方式から独立した業務モデル。

use core::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Note {
    pub note_id: NoteId,
    pub creator_issuer: String,
    pub creator_subject: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub created_at: UnixMillis,
    pub updated_at: UnixMillis,
    pub revision: i64,
    pub deleted_at: Option<UnixMillis>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteDraft {
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteAclEntry {
    pub issuer: String,
    pub subject: String,
    pub permission: NotePermission,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NoteBundle {
    pub note: Note,
    pub acl: Vec<NoteAclEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Archive {
    pub format: String,
    pub notes: Vec<NoteBundle>,
}

pub const ARCHIVE_FORMAT: &str = "marginalis-archive-1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Actor {
    pub issuer: String,
    pub subject: String,
    pub is_administrator: bool,
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
pub struct McpAuthorizationGrant {
    pub actor: Actor,
    pub client_id: String,
    pub redirect_uri: String,
    pub resource_uri: String,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAuthenticatedActor {
    pub actor: Actor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpOAuthClient {
    pub client_id: String,
    pub display_name: String,
    pub redirect_uris: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
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

