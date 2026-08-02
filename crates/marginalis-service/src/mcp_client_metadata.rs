//! Client ID Metadata Documentを安全な境界内で取得するadapter。

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use marginalis_application::{McpClientMetadataResolver, McpOAuthRepositoryError};
use marginalis_domain::McpOAuthClient;
use serde::Deserialize;

const MAX_METADATA_BYTES: usize = 5 * 1024;

pub(crate) struct HttpMcpClientMetadataResolver {
    timeout: Duration,
    cache: tokio::sync::Mutex<HashMap<String, CachedClientMetadata>>,
}

struct CachedClientMetadata {
    client: McpOAuthClient,
    expires_at: Instant,
}

impl HttpMcpClientMetadataResolver {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Deserialize)]
struct ClientMetadataDocument {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
    token_endpoint_auth_method: Option<String>,
    grant_types: Option<Vec<String>>,
    response_types: Option<Vec<String>>,
}

#[async_trait]
impl McpClientMetadataResolver for HttpMcpClientMetadataResolver {
    async fn resolve(
        &self,
        client_id: &str,
    ) -> Result<Option<McpOAuthClient>, McpOAuthRepositoryError> {
        {
            let mut cache = self.cache.lock().await;
            cache.retain(|_, entry| entry.expires_at > Instant::now());
            if let Some(entry) = cache.get(client_id) {
                return Ok(Some(entry.client.clone()));
            }
        }
        let url = url::Url::parse(client_id).map_err(|_| McpOAuthRepositoryError)?;
        let host = url.host_str().ok_or(McpOAuthRepositoryError)?;
        let port = url.port_or_known_default().ok_or(McpOAuthRepositoryError)?;
        let addresses = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| McpOAuthRepositoryError)?
            .collect::<Vec<_>>();
        if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
            return Ok(None);
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(self.timeout)
            .no_proxy()
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| McpOAuthRepositoryError)?;
        let mut response = client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| McpOAuthRepositoryError)?;
        if response.status() != reqwest::StatusCode::OK {
            return Ok(None);
        }
        let cache_lifetime = cache_lifetime(response.headers());
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| {
                let value = value.trim();
                value.eq_ignore_ascii_case("application/json")
                    || (value.to_ascii_lowercase().starts_with("application/")
                        && value.to_ascii_lowercase().ends_with("+json"))
            })
        {
            return Ok(None);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_METADATA_BYTES as u64)
        {
            return Ok(None);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| McpOAuthRepositoryError)?
        {
            if body.len().saturating_add(chunk.len()) > MAX_METADATA_BYTES {
                return Ok(None);
            }
            body.extend_from_slice(&chunk);
        }
        let Some(client) = parse_client_metadata(client_id, &body) else {
            return Ok(None);
        };
        if let Some(lifetime) = cache_lifetime {
            self.cache.lock().await.insert(
                client_id.to_owned(),
                CachedClientMetadata {
                    client: client.clone(),
                    expires_at: Instant::now() + lifetime,
                },
            );
        }
        Ok(Some(client))
    }
}

fn parse_client_metadata(client_id: &str, body: &[u8]) -> Option<McpOAuthClient> {
    let document = serde_json::from_slice::<ClientMetadataDocument>(body).ok()?;
    if document.client_id != client_id
        || document.token_endpoint_auth_method.as_deref() != Some("none")
        || document
            .grant_types
            .as_ref()
            .is_some_and(|values| !values.iter().any(|value| value == "authorization_code"))
        || document
            .response_types
            .as_ref()
            .is_some_and(|values| !values.iter().any(|value| value == "code"))
    {
        return None;
    }
    Some(McpOAuthClient {
        client_id: document.client_id,
        display_name: document.client_name,
        redirect_uris: document.redirect_uris,
    })
}

fn cache_lifetime(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let directives = headers
        .get_all(reqwest::header::CACHE_CONTROL)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .collect::<Vec<_>>();
    if directives.iter().any(|directive| {
        directive.eq_ignore_ascii_case("no-store") || directive.eq_ignore_ascii_case("no-cache")
    }) {
        return None;
    }
    let maximum_age = directives.iter().find_map(|directive| {
        let (name, value) = directive.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("max-age")
            .then(|| value.trim_matches('"').parse::<u64>().ok())
            .flatten()
    })?;
    let current_age = headers
        .get(reqwest::header::AGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let remaining = maximum_age.saturating_sub(current_age).min(60 * 60);
    (remaining > 0).then(|| Duration::from_secs(remaining))
}

fn public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => public_ipv4(address),
        IpAddr::V6(address) => public_ipv6(address),
    }
}

fn public_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    !(address.is_unspecified()
        || address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_multicast()
        || address.is_broadcast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 31 && octets[2] == 196)
        || (octets[0] == 192 && octets[1] == 52 && octets[2] == 193)
        || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
        || (octets[0] == 192 && octets[1] == 175 && octets[2] == 48)
        || (octets[0] == 198 && matches!(octets[1], 18 | 19))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        || octets[0] >= 240)
}

fn public_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    let special_prefix = |prefix: Ipv6Addr, bits: u32| {
        let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
        (u128::from(address) & mask) == (u128::from(prefix) & mask)
    };
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || special_prefix("::".parse().expect("constant IPv6"), 96)
        || special_prefix("::ffff:0:0".parse().expect("constant IPv6"), 96)
        || special_prefix("64:ff9b::".parse().expect("constant IPv6"), 96)
        || special_prefix("64:ff9b:1::".parse().expect("constant IPv6"), 48)
        || special_prefix("100::".parse().expect("constant IPv6"), 64)
        || special_prefix("2001::".parse().expect("constant IPv6"), 23)
        || special_prefix("2001:db8::".parse().expect("constant IPv6"), 32)
        || special_prefix("2002::".parse().expect("constant IPv6"), 16)
        || special_prefix("2620:4f:8000::".parse().expect("constant IPv6"), 48)
        || special_prefix("3fff::".parse().expect("constant IPv6"), 20)
        || special_prefix("5f00::".parse().expect("constant IPv6"), 16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_fetch_rejects_non_public_destinations() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "192.0.2.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "100::1",
            "2001::1",
            "2002::1",
            "::ffff:8.8.8.8",
            "::8.8.8.8",
            "2620:4f:8000::1",
        ] {
            assert!(
                !public_ip(address.parse().expect("IP address")),
                "{address}"
            );
        }
        assert!(public_ip("8.8.8.8".parse().expect("public IPv4")));
        assert!(public_ip(
            "2606:4700:4700::1111".parse().expect("public IPv6")
        ));
    }

    #[test]
    fn metadata_cache_respects_cache_control_and_age() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CACHE_CONTROL,
            "public, max-age=120".parse().expect("cache control"),
        );
        headers.insert(reqwest::header::AGE, "20".parse().expect("age"));
        assert_eq!(cache_lifetime(&headers), Some(Duration::from_secs(100)));

        headers.insert(
            reqwest::header::CACHE_CONTROL,
            "no-cache, max-age=120".parse().expect("cache control"),
        );
        assert_eq!(cache_lifetime(&headers), None);
    }

    #[test]
    fn client_metadata_requires_an_exact_id_and_public_client_method() {
        let client_id = "https://client.example/oauth/metadata.json";
        let valid = br#"{
            "client_id":"https://client.example/oauth/metadata.json",
            "client_name":"Example client",
            "redirect_uris":["http://127.0.0.1/callback"],
            "grant_types":["authorization_code"],
            "response_types":["code"],
            "token_endpoint_auth_method":"none"
        }"#;
        assert!(parse_client_metadata(client_id, valid).is_some());
        assert!(parse_client_metadata("https://other.example/metadata.json", valid).is_none());

        let confidential = valid
            .windows(b"none".len())
            .position(|window| window == b"none")
            .map(|position| {
                let mut value = valid.to_vec();
                value.splice(
                    position..position + b"none".len(),
                    b"client_secret_basic".iter().copied(),
                );
                value
            })
            .expect("authentication method");
        assert!(parse_client_metadata(client_id, &confidential).is_none());
    }
}
