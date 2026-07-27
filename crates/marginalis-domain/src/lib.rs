//! Marginalisの永続化方式から独立した業務モデル。

use core::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use url::Url;
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub format: String,
    pub adocweave_package_version: String,
    pub note_profile_version: u32,
    pub notes: Vec<Note>,
}

pub const ARCHIVE_FORMAT: &str = "marginalis-archive-4";
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
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpOAuthClient {
    pub client_id: String,
    pub display_name: String,
    pub redirect_uris: Vec<String>,
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
        assert_eq!(validate_identity(&long_issuer, "alice"), Err(InvalidIdentity));
        assert_eq!(
            validate_identity(
                "https://id.example.test",
                &"a".repeat(MAX_IDENTITY_SUBJECT_BYTES + 1)
            ),
            Err(InvalidIdentity)
        );
    }
}
