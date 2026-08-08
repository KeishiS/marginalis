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
}
