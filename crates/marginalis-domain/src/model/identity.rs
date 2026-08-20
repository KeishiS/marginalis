//! 外部identity providerが発行した主体識別子と、それを持つ操作主体・session。

use url::Url;

use super::UnixMillis;

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

/// Marginalis内部で一人の利用者を識別する、SQLite由来の正の整数。
///
/// 公開API、archive、ログには出さない。
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PrincipalId(i64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("a principal ID must be a positive integer")]
pub struct InvalidPrincipalId;

impl PrincipalId {
    pub const fn new(value: i64) -> Result<Self, InvalidPrincipalId> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(InvalidPrincipalId)
        }
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

/// 業務データが保持するprincipalと、公開時に使う代表identity。
#[derive(Clone, Debug, Eq)]
pub struct PrincipalRef {
    id: PrincipalId,
    primary_identity: Identity,
}

impl PartialEq for PrincipalRef {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::hash::Hash for PrincipalRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PrincipalRef {
    pub const fn new(id: PrincipalId, primary_identity: Identity) -> Self {
        Self {
            id,
            primary_identity,
        }
    }

    pub const fn id(&self) -> PrincipalId {
        self.id
    }

    pub const fn primary_identity(&self) -> &Identity {
        &self.primary_identity
    }
}

/// 保存済みのprincipalと、それに属する検証済み外部identityの集合。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Principal {
    reference: PrincipalRef,
    identities: Vec<Identity>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("principal identities are inconsistent")]
pub struct InvalidPrincipal;

impl Principal {
    pub fn restore(
        id: PrincipalId,
        primary_identity: Identity,
        identities: Vec<Identity>,
    ) -> Result<Self, InvalidPrincipal> {
        if identities.is_empty()
            || !identities.contains(&primary_identity)
            || identities
                .iter()
                .enumerate()
                .any(|(index, identity)| identities[index + 1..].contains(identity))
        {
            return Err(InvalidPrincipal);
        }
        Ok(Self {
            reference: PrincipalRef::new(id, primary_identity),
            identities,
        })
    }

    pub fn single(id: PrincipalId, identity: Identity) -> Self {
        Self {
            reference: PrincipalRef::new(id, identity.clone()),
            identities: vec![identity],
        }
    }

    pub const fn reference(&self) -> &PrincipalRef {
        &self.reference
    }

    pub fn contains(&self, identity: &Identity) -> bool {
        self.identities.contains(identity)
    }
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
    principal: PrincipalRef,
    authenticated_identity: Identity,
}

impl Actor {
    pub fn authenticate(
        principal: Principal,
        authenticated_identity: Identity,
    ) -> Result<Self, InvalidPrincipal> {
        if !principal.contains(&authenticated_identity) {
            return Err(InvalidPrincipal);
        }
        Ok(Self {
            principal: principal.reference,
            authenticated_identity,
        })
    }

    /// identityが一つだけのprincipalを作るための補助constructor。
    ///
    /// 永続化済みidentityの解決には`PrincipalDirectory`を使う。
    pub fn for_single_identity(principal_id: PrincipalId, identity: Identity) -> Self {
        Self {
            principal: PrincipalRef::new(principal_id, identity.clone()),
            authenticated_identity: identity,
        }
    }

    pub const fn principal(&self) -> &PrincipalRef {
        &self.principal
    }

    pub const fn principal_id(&self) -> PrincipalId {
        self.principal.id()
    }

    pub const fn authenticated_identity(&self) -> &Identity {
        &self.authenticated_identity
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
    fn principal_ids_are_positive_and_never_equal_by_display_identity() {
        assert!(PrincipalId::new(0).is_err());
        assert!(PrincipalId::new(-1).is_err());
        let identity =
            Identity::new("https://id.example.test".into(), "alice".into()).expect("identity");
        assert_ne!(
            PrincipalRef::new(PrincipalId::new(1).expect("ID"), identity.clone()),
            PrincipalRef::new(PrincipalId::new(2).expect("ID"), identity),
        );
    }

    #[test]
    fn actor_rejects_an_identity_not_bound_to_the_principal() {
        let alice = Identity::new("https://id.example.test".into(), "alice".into()).expect("alice");
        let bob = Identity::new("https://id.example.test".into(), "bob".into()).expect("bob");
        let principal = Principal::single(PrincipalId::new(1).expect("ID"), alice);
        assert_eq!(Actor::authenticate(principal, bob), Err(InvalidPrincipal));
    }
}
