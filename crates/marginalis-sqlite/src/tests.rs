use std::str::FromStr;

use marginalis_application::{
    BibliographyRepository, BibliographyRepositoryError, MathMacro, MathMacroRepository,
    MathMacroRepositoryError, McpAuthorizationCodeExchange, McpRefreshTokenRotation,
    McpRefreshTokenRotationOutcome, OidcLoginAttempt, OidcLoginAttemptStore, RestorePlan,
};
use marginalis_domain::{
    Actor, BibliographyItem, BibliographyItemId, EntityId, Identity, McpAuthorizationGrant,
    McpOAuthClient, McpResolvedRedirectUri, Note, NoteAccess, NoteAclEntry, NoteDraft, NoteId,
    NotePermission, Revision, SOFT_DELETE_RETENTION_MS, UnixMillis, WebSession,
};

use super::*;

fn actor(issuer: &str, subject: &str) -> Actor {
    Actor::try_new(issuer.into(), subject.into()).expect("valid test actor")
}

fn revision(value: i64) -> Revision {
    Revision::new(value).expect("positive test revision")
}

mod schema;

mod notes;

mod bibliography;

mod math_macros;

mod sessions;

mod oauth;
