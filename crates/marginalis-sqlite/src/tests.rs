use std::str::FromStr;

use marginalis_application::{
    McpAuthorizationCodeExchange, McpRefreshTokenRotation, McpRefreshTokenRotationOutcome,
    OidcLoginAttempt, OidcLoginAttemptStore, RestorePlan,
};
use marginalis_domain::{
    Actor, EntityId, Identity, McpAuthorizationGrant, McpOAuthClient, Note, NoteAccess,
    NoteAclEntry, NoteDraft, NoteId, NotePermission, Revision, SOFT_DELETE_RETENTION_MS,
    UnixMillis, WebSession,
};

use super::*;

fn actor(issuer: &str, subject: &str) -> Actor {
    Actor::try_new(issuer.into(), subject.into()).expect("valid test actor")
}

fn revision(value: i64) -> Revision {
    Revision::new(value).expect("positive test revision")
}

mod schema {
    use super::*;

    include!("tests/schema.rs");
}

mod notes {
    use super::*;

    include!("tests/notes.rs");
}

mod sessions {
    use super::*;

    include!("tests/sessions.rs");
}

mod oauth {
    use super::*;

    include!("tests/oauth.rs");
}
