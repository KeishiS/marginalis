use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use url::Url;

use crate::Client;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourcePolicyError {
    InvalidResourceUri,
    InvalidDisplayName,
    InvalidScopes,
}

impl std::fmt::Display for ResourcePolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidResourceUri => "resource URI is invalid",
            Self::InvalidDisplayName => "resource display name is invalid",
            Self::InvalidScopes => "resource scopes are invalid",
        })
    }
}

impl std::error::Error for ResourcePolicyError {}

/// 一つのMCP resourceに対して公開し、発行できるscopeの正本。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourcePolicy {
    uri: Url,
    display_name: String,
    supported_scopes: Vec<String>,
    default_scopes: Vec<String>,
}

impl ResourcePolicy {
    pub fn new(
        uri: String,
        display_name: String,
        supported_scopes: Vec<String>,
        default_scopes: Vec<String>,
    ) -> Result<Self, ResourcePolicyError> {
        let uri = Url::parse(&uri).map_err(|_| ResourcePolicyError::InvalidResourceUri)?;
        if !matches!(uri.scheme(), "http" | "https") || uri.host().is_none() {
            return Err(ResourcePolicyError::InvalidResourceUri);
        }
        if display_name.trim().is_empty()
            || display_name.len() > 128
            || display_name.chars().any(|character| {
                character.is_control()
                    || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
        {
            return Err(ResourcePolicyError::InvalidDisplayName);
        }
        if supported_scopes.is_empty()
            || supported_scopes.iter().any(|scope| !valid_scope(scope))
            || has_duplicates(&supported_scopes)
            || default_scopes.is_empty()
            || has_duplicates(&default_scopes)
            || default_scopes
                .iter()
                .any(|scope| !supported_scopes.contains(scope))
        {
            return Err(ResourcePolicyError::InvalidScopes);
        }
        let supported_scopes = canonical_scopes(&supported_scopes, &supported_scopes);
        let default_scopes = canonical_scopes(&default_scopes, &supported_scopes);
        Ok(Self {
            uri,
            display_name,
            supported_scopes,
            default_scopes,
        })
    }

    pub fn uri(&self) -> &Url {
        &self.uri
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn supported_scopes(&self) -> &[String] {
        &self.supported_scopes
    }

    pub fn default_scopes(&self) -> &[String] {
        &self.default_scopes
    }

    pub fn resource_uri_matches(&self, received: &str) -> bool {
        Url::parse(received).is_ok_and(|received| received == self.uri)
    }

    pub fn resolve_scopes(&self, requested: &[String]) -> Option<Vec<String>> {
        if requested.is_empty() {
            return Some(self.default_scopes.clone());
        }
        requested
            .iter()
            .all(|scope| self.supported_scopes.contains(scope))
            .then(|| canonical_scopes(requested, &self.supported_scopes))
    }
}

pub fn canonical_scopes(scopes: &[String], supported_scopes: &[String]) -> Vec<String> {
    supported_scopes
        .iter()
        .filter(|candidate| scopes.contains(candidate))
        .cloned()
        .collect()
}

pub fn pkce_s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn valid_pkce_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub fn valid_pkce_verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

pub fn validate_client_metadata(client: &Client) -> Result<(), AuthorizationClientError> {
    if client.client_id.is_empty()
        || client.client_id.len() > 2_048
        || client.display_name.trim().is_empty()
        || client.redirect_uris.is_empty()
        || client.display_name.len() > 128
        || client.redirect_uris.len() > 8
        || client.display_name.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return Err(AuthorizationClientError::InvalidMetadata);
    }
    if !client
        .redirect_uris
        .iter()
        .all(|uri| uri.len() <= 2_048 && valid_redirect_uri(uri))
    {
        return Err(AuthorizationClientError::InvalidRedirectUri);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorizationClientError {
    InvalidMetadata,
    InvalidRedirectUri,
}

pub fn valid_client_metadata_document_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    let Some(authority_end) = value
        .strip_prefix("https://")
        .and_then(|remainder| remainder.find('/').map(|index| index + "https://".len()))
    else {
        return false;
    };
    let raw_path = value[authority_end..]
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    let has_dot_segment = raw_path.split('/').any(|segment| {
        matches!(
            segment.to_ascii_lowercase().as_str(),
            "." | ".." | "%2e" | ".%2e" | "%2e." | "%2e%2e"
        )
    });
    value.len() <= 2_048
        && url.scheme() == "https"
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && !raw_path.is_empty()
        && !has_dot_segment
}

pub fn redirect_uri_matches(registered: &str, requested: &str) -> bool {
    if registered == requested {
        return true;
    }
    let (Ok(mut registered), Ok(mut requested)) = (Url::parse(registered), Url::parse(requested))
    else {
        return false;
    };
    if registered.scheme() != "http"
        || requested.scheme() != "http"
        || !is_loopback_host(&registered)
        || !is_loopback_host(&requested)
        || registered.host() != requested.host()
    {
        return false;
    }
    let _ = registered.set_port(None);
    let _ = requested.set_port(None);
    registered == requested
}

fn valid_scope(scope: &str) -> bool {
    !scope.is_empty()
        && scope.len() <= 128
        && scope
            .bytes()
            .all(|byte| matches!(byte, 0x21 | 0x23..=0x5b | 0x5d..=0x7e))
}

fn has_duplicates(values: &[String]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}

pub fn valid_redirect_uri(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if url.host().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    if url.scheme() == "https" {
        return true;
    }
    url.scheme() == "http" && is_loopback_host(&url)
}

fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> ResourcePolicy {
        ResourcePolicy::new(
            "https://notes.example.test/mcp".into(),
            "Example notes".into(),
            vec![
                "notes:read".into(),
                "notes:write".into(),
                "notes:delete".into(),
            ],
            vec!["notes:read".into()],
        )
        .expect("valid policy")
    }

    #[test]
    fn policy_is_the_scope_and_resource_source_of_truth() {
        let policy = policy();
        assert!(policy.resource_uri_matches("HTTPS://Notes.Example.Test/mcp"));
        assert_eq!(policy.resolve_scopes(&[]), Some(vec!["notes:read".into()]));
        assert_eq!(
            policy.resolve_scopes(&["notes:delete".into(), "notes:read".into()]),
            Some(vec!["notes:read".into(), "notes:delete".into()])
        );
        assert_eq!(policy.resolve_scopes(&["other:read".into()]), None);
        assert_eq!(
            ResourcePolicy::new(
                "https://notes.example.test/mcp".into(),
                "Example notes".into(),
                vec!["notes:read".into(), "notes:read".into()],
                vec!["notes:read".into()],
            ),
            Err(ResourcePolicyError::InvalidScopes)
        );
    }

    #[test]
    fn redirects_require_https_or_a_loopback_host() {
        for valid in [
            "https://chatgpt.com/connector/oauth/callback",
            "http://localhost/callback",
            "http://localhost:48123/callback",
            "http://127.0.0.1/callback",
            "http://127.0.0.1:48123/callback",
            "http://[::1]:48123/callback",
        ] {
            let client = Client {
                client_id: "client".into(),
                display_name: "Client".into(),
                redirect_uris: vec![valid.into()],
            };
            assert_eq!(validate_client_metadata(&client), Ok(()), "{valid}");
        }
        for invalid in [
            "http://localhost.example:48123/callback",
            "http://client.example.test/callback",
            "https://client.example.test/callback?next=other",
            "https://user@client.example.test/callback",
        ] {
            let client = Client {
                client_id: "client".into(),
                display_name: "Client".into(),
                redirect_uris: vec![invalid.into()],
            };
            assert_eq!(
                validate_client_metadata(&client),
                Err(AuthorizationClientError::InvalidRedirectUri),
                "{invalid}"
            );
        }
        assert!(redirect_uri_matches(
            "http://127.0.0.1/callback",
            "http://127.0.0.1:49152/callback"
        ));
        assert!(!redirect_uri_matches(
            "http://127.0.0.1/callback",
            "http://127.0.0.1:49152/other"
        ));
    }

    #[test]
    fn client_metadata_document_url_has_a_safe_https_path() {
        assert!(valid_client_metadata_document_url(
            "https://client.example/oauth/metadata.json"
        ));
        for invalid in [
            "https://client.example",
            "https://user@client.example/metadata.json",
            "https://client.example/a/../metadata.json",
            "https://client.example/a/%2e%2e/metadata.json",
            "https://client.example/metadata.json?version=1",
            "https://client.example/metadata.json#client",
        ] {
            assert!(!valid_client_metadata_document_url(invalid), "{invalid}");
        }
    }
}
