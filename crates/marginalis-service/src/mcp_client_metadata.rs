//! Client ID Metadata Documentを安全な境界内で取得するadapter。

use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use marginalis_application::{McpClientMetadataResolver, McpOAuthRepositoryError};
use marginalis_domain::McpOAuthClient;
use serde::Deserialize;

const MAX_METADATA_BYTES: usize = 5 * 1024;
/// 一定時間に実行してよい外部取得の回数。
///
/// `/oauth/authorize`はログイン前に到達できるため、未認証の第三者が任意の公開hostへ要求を
/// 出させられる。取得そのものに上限を設け、cacheに載る正規のclientは上限を消費しない。
const MAX_FETCHES_PER_WINDOW: usize = 60;
const FETCH_WINDOW: Duration = Duration::from_secs(60);
/// 取得できなかったclient IDを覚えておく時間。同じ値の繰り返しで取得が増えないようにする。
const REJECTION_LIFETIME: Duration = Duration::from_secs(60);
/// cacheの上限。攻撃者がclient IDを変えながら要求してもメモリーが増え続けないようにする。
const MAX_CACHED_CLIENTS: usize = 1_024;
const MAX_CACHED_REJECTIONS: usize = 1_024;

pub(crate) struct HttpMcpClientMetadataResolver {
    timeout: Duration,
    state: tokio::sync::Mutex<ResolverState>,
}

#[derive(Default)]
struct ResolverState {
    resolved: HashMap<String, CachedClientMetadata>,
    rejected: HashMap<String, Instant>,
    fetches: VecDeque<Instant>,
}

/// cacheを引いた結果と、取得してよいかどうか。
enum FetchPermit {
    Resolved(McpOAuthClient),
    Rejected,
    Allowed,
}

struct CachedClientMetadata {
    client: McpOAuthClient,
    expires_at: Instant,
}

impl HttpMcpClientMetadataResolver {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            state: tokio::sync::Mutex::new(ResolverState::default()),
        }
    }

    /// cacheを引き、取得が必要な場合は回数の枠を確保する。
    ///
    /// 枠を使い切っている場合はErrを返す。呼び出し元はこれを一時的な障害として扱い、
    /// clientが不正であるかのようには伝えない。
    async fn begin_fetch(&self, client_id: &str) -> Result<FetchPermit, McpOAuthRepositoryError> {
        let now = Instant::now();
        let mut state = self.state.lock().await;
        state.resolved.retain(|_, entry| entry.expires_at > now);
        state
            .rejected
            .retain(|_, rejected_at| now.duration_since(*rejected_at) < REJECTION_LIFETIME);
        let cutoff = now.checked_sub(FETCH_WINDOW).unwrap_or(now);
        while state.fetches.front().is_some_and(|at| *at <= cutoff) {
            state.fetches.pop_front();
        }
        if let Some(entry) = state.resolved.get(client_id) {
            return Ok(FetchPermit::Resolved(entry.client.clone()));
        }
        if state.rejected.contains_key(client_id) {
            return Ok(FetchPermit::Rejected);
        }
        if state.fetches.len() >= MAX_FETCHES_PER_WINDOW {
            tracing::warn!(
                event = "mcp.oauth.client_metadata.throttled",
                "MCP client metadata fetch was throttled"
            );
            return Err(McpOAuthRepositoryError);
        }
        state.fetches.push_back(now);
        Ok(FetchPermit::Allowed)
    }

    async fn remember_resolved(
        &self,
        client_id: &str,
        client: &McpOAuthClient,
        lifetime: Duration,
    ) {
        let mut state = self.state.lock().await;
        if state.resolved.len() >= MAX_CACHED_CLIENTS && !state.resolved.contains_key(client_id) {
            return;
        }
        state.resolved.insert(
            client_id.to_owned(),
            CachedClientMetadata {
                client: client.clone(),
                expires_at: Instant::now() + lifetime,
            },
        );
    }

    async fn remember_rejected(&self, client_id: &str) {
        let mut state = self.state.lock().await;
        if state.rejected.len() >= MAX_CACHED_REJECTIONS && !state.rejected.contains_key(client_id)
        {
            return;
        }
        state.rejected.insert(client_id.to_owned(), Instant::now());
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
        match self.begin_fetch(client_id).await? {
            FetchPermit::Resolved(client) => return Ok(Some(client)),
            FetchPermit::Rejected => return Ok(None),
            FetchPermit::Allowed => {}
        }
        let Some((client, cache_lifetime)) = self.fetch(client_id).await? else {
            self.remember_rejected(client_id).await;
            return Ok(None);
        };
        if let Some(lifetime) = cache_lifetime {
            self.remember_resolved(client_id, &client, lifetime).await;
        }
        Ok(Some(client))
    }
}

impl HttpMcpClientMetadataResolver {
    /// 文書を取得して検査する。取得先や内容が条件を満たさない場合は`None`を返す。
    #[allow(clippy::type_complexity)]
    async fn fetch(
        &self,
        client_id: &str,
    ) -> Result<Option<(McpOAuthClient, Option<Duration>)>, McpOAuthRepositoryError> {
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
        Ok(Some((client, cache_lifetime)))
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

    /// 未認証で到達できる経路のため、外部取得の回数そのものに上限を設ける。
    #[tokio::test]
    async fn metadata_fetches_are_bounded_per_window() {
        let resolver = HttpMcpClientMetadataResolver::new(Duration::from_secs(1));
        for index in 0..MAX_FETCHES_PER_WINDOW {
            assert!(
                matches!(
                    resolver
                        .begin_fetch(&format!("https://client{index}.example/metadata.json"))
                        .await,
                    Ok(FetchPermit::Allowed)
                ),
                "{index}回目までは取得を許す"
            );
        }
        assert!(
            resolver
                .begin_fetch("https://overflow.example/metadata.json")
                .await
                .is_err(),
            "上限を超えた取得は一時的な障害として扱う"
        );
    }

    /// 同じclient IDの繰り返しで取得が増えないことを確認する。
    #[tokio::test]
    async fn rejected_client_ids_are_not_fetched_again() {
        let resolver = HttpMcpClientMetadataResolver::new(Duration::from_secs(1));
        let client_id = "https://client.example/metadata.json";
        assert!(matches!(
            resolver.begin_fetch(client_id).await,
            Ok(FetchPermit::Allowed)
        ));
        resolver.remember_rejected(client_id).await;
        for _ in 0..MAX_FETCHES_PER_WINDOW {
            assert!(matches!(
                resolver.begin_fetch(client_id).await,
                Ok(FetchPermit::Rejected)
            ));
        }
        assert!(
            matches!(
                resolver
                    .begin_fetch("https://other.example/metadata.json")
                    .await,
                Ok(FetchPermit::Allowed)
            ),
            "拒否の記録は取得の枠を消費しない"
        );
    }

    /// 取得できたclientはcacheから返し、取得の枠を消費しない。
    #[tokio::test]
    async fn cached_clients_do_not_consume_the_fetch_budget() {
        let resolver = HttpMcpClientMetadataResolver::new(Duration::from_secs(1));
        let client_id = "https://client.example/metadata.json";
        let client = McpOAuthClient {
            client_id: client_id.into(),
            display_name: "Example client".into(),
            redirect_uris: vec!["https://client.example/callback".into()],
        };
        resolver
            .remember_resolved(client_id, &client, Duration::from_secs(600))
            .await;
        for _ in 0..MAX_FETCHES_PER_WINDOW + 1 {
            assert!(matches!(
                resolver.begin_fetch(client_id).await,
                Ok(FetchPermit::Resolved(resolved)) if resolved == client
            ));
        }
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
