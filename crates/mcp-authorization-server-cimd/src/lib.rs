//! Client ID Metadata Documentを安全な境界内で取得するadapter。

use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use mcp_authorization_server::{Client, ClientMetadataResolver, ClientMetadataResolverError};
use serde::Deserialize;

const MAX_METADATA_BYTES: usize = 5 * 1024;
/// 一定時間に実行してよい外部取得の回数。
///
/// `/oauth/authorize`はログイン前に到達できるため、未認証の第三者が任意の公開hostへ要求を
/// 出させられる。取得そのものに上限を設け、cacheに載る正規のclientは上限を消費しない。
const MAX_FETCHES_PER_WINDOW: usize = 60;
const FETCH_WINDOW: Duration = Duration::from_secs(60);
const MAX_CONCURRENT_FETCHES: usize = 8;
/// cacheの上限。攻撃者がclient IDを変えながら要求してもメモリーが増え続けないようにする。
const MAX_CACHED_CLIENTS: usize = 1_024;
const MAX_CLIENT_FLIGHTS: usize = 1_024;

type FetchResult = Result<Option<Client>, ClientMetadataResolverError>;
type ClientFlight = tokio::sync::OnceCell<FetchResult>;

pub struct HttpClientMetadataResolver {
    timeout: Duration,
    state: tokio::sync::Mutex<ResolverState>,
    fetch_slots: tokio::sync::Semaphore,
    client_flights: tokio::sync::Mutex<HashMap<String, Weak<ClientFlight>>>,
}

#[derive(Default)]
struct ResolverState {
    resolved: HashMap<String, CachedClientMetadata>,
    fetches: VecDeque<Instant>,
}

/// cacheを引いた結果と、取得してよいかどうか。
enum FetchPermit {
    Resolved(Client),
    Allowed,
}

struct CachedClientMetadata {
    client: Client,
    expires_at: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClientMetadataFailure {
    RateLimit,
    SingleFlightCapacity,
    FetchSlotClosed,
    Timeout,
    ClientIdUrl,
    DnsLookup,
    NonPublicAddress,
    HttpClient,
    HttpRequest,
    HttpStatus(u16),
    ContentType,
    ResponseTooLarge,
    ResponseBody,
    DocumentFormat,
    ClientIdMismatch,
    AuthenticationMethodConflict,
    AuthenticationMethodUnsupported,
    GrantType,
    ResponseType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClientMetadataFailureDisposition {
    Rejected,
    Unavailable,
    Throttled,
}

impl ClientMetadataFailure {
    fn disposition(self) -> ClientMetadataFailureDisposition {
        match self {
            Self::RateLimit => ClientMetadataFailureDisposition::Throttled,
            Self::SingleFlightCapacity
            | Self::FetchSlotClosed
            | Self::Timeout
            | Self::ClientIdUrl
            | Self::DnsLookup
            | Self::HttpClient
            | Self::HttpRequest
            | Self::ResponseBody => ClientMetadataFailureDisposition::Unavailable,
            Self::HttpStatus(408 | 429 | 500..=599) => {
                ClientMetadataFailureDisposition::Unavailable
            }
            Self::NonPublicAddress
            | Self::HttpStatus(_)
            | Self::ContentType
            | Self::ResponseTooLarge
            | Self::DocumentFormat
            | Self::ClientIdMismatch
            | Self::AuthenticationMethodConflict
            | Self::AuthenticationMethodUnsupported
            | Self::GrantType
            | Self::ResponseType => ClientMetadataFailureDisposition::Rejected,
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Self::RateLimit => "rate-limit",
            Self::SingleFlightCapacity => "single-flight-capacity",
            Self::FetchSlotClosed => "fetch-slot-closed",
            Self::Timeout => "timeout",
            Self::ClientIdUrl => "client-id-url",
            Self::DnsLookup => "dns-lookup",
            Self::NonPublicAddress => "non-public-address",
            Self::HttpClient => "http-client",
            Self::HttpRequest => "http-request",
            Self::HttpStatus(_) => "http-status",
            Self::ContentType => "content-type",
            Self::ResponseTooLarge => "response-too-large",
            Self::ResponseBody => "response-body",
            Self::DocumentFormat => "document-format",
            Self::ClientIdMismatch => "client-id-mismatch",
            Self::AuthenticationMethodConflict => "authentication-method-conflict",
            Self::AuthenticationMethodUnsupported => "authentication-method-unsupported",
            Self::GrantType => "grant-type",
            Self::ResponseType => "response-type",
        }
    }
}

impl HttpClientMetadataResolver {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            state: tokio::sync::Mutex::new(ResolverState::default()),
            fetch_slots: tokio::sync::Semaphore::new(MAX_CONCURRENT_FETCHES),
            client_flights: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// cacheを引き、取得が必要な場合は回数の枠を確保する。
    ///
    /// 枠を使い切っている場合はErrを返す。呼び出し元はこれを一時的な障害として扱い、
    /// clientが不正であるかのようには伝えない。
    async fn begin_fetch(&self, client_id: &str) -> Result<FetchPermit, ClientMetadataFailure> {
        let now = Instant::now();
        let mut state = self.state.lock().await;
        state.resolved.retain(|_, entry| entry.expires_at > now);
        let cutoff = now.checked_sub(FETCH_WINDOW).unwrap_or(now);
        while state.fetches.front().is_some_and(|at| *at <= cutoff) {
            state.fetches.pop_front();
        }
        if let Some(entry) = state.resolved.get(client_id) {
            return Ok(FetchPermit::Resolved(entry.client.clone()));
        }
        if state.fetches.len() >= MAX_FETCHES_PER_WINDOW {
            return Err(ClientMetadataFailure::RateLimit);
        }
        state.fetches.push_back(now);
        Ok(FetchPermit::Allowed)
    }

    /// 同じclient IDへの同時要求が一つの取得結果を共有するための状態を返す。
    ///
    /// mapは弱い参照だけを保持するため、同時要求がなくなれば成功・失敗のどちらも残らない。
    async fn client_flight(
        &self,
        client_id: &str,
    ) -> Result<Arc<ClientFlight>, ClientMetadataFailure> {
        let mut flights = self.client_flights.lock().await;
        flights.retain(|_, flight| flight.strong_count() > 0);
        if let Some(flight) = flights.get(client_id).and_then(Weak::upgrade) {
            return Ok(flight);
        }
        if flights.len() >= MAX_CLIENT_FLIGHTS {
            return Err(ClientMetadataFailure::SingleFlightCapacity);
        }
        let flight = Arc::new(ClientFlight::new());
        flights.insert(client_id.to_owned(), Arc::downgrade(&flight));
        Ok(flight)
    }

    async fn remember_resolved(&self, client_id: &str, client: &Client, lifetime: Duration) {
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

    async fn fetch_once(&self, client_id: &str) -> FetchResult {
        match self.begin_fetch(client_id).await {
            Err(failure) => return metadata_failure_result(client_id, failure),
            Ok(FetchPermit::Resolved(client)) => return Ok(Some(client)),
            Ok(FetchPermit::Allowed) => {}
        }
        let _fetch_slot = match self.fetch_slots.acquire().await {
            Ok(slot) => slot,
            Err(_) => {
                return metadata_failure_result(client_id, ClientMetadataFailure::FetchSlotClosed);
            }
        };
        let fetched = match tokio::time::timeout(self.timeout, self.fetch(client_id)).await {
            Ok(result) => result,
            Err(_) => Err(ClientMetadataFailure::Timeout),
        };
        let (client, cache_lifetime) = match fetched {
            Ok(fetched) => fetched,
            Err(failure) => return metadata_failure_result(client_id, failure),
        };
        if let Some(lifetime) = cache_lifetime {
            self.remember_resolved(client_id, &client, lifetime).await;
        }
        Ok(Some(client))
    }
}

#[derive(Deserialize)]
struct ClientMetadataDocument {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
    token_endpoint_auth_method: Option<String>,
    token_endpoint_auth_methods_supported: Option<Vec<String>>,
    grant_types: Option<Vec<String>>,
    response_types: Option<Vec<String>>,
}

#[async_trait]
impl ClientMetadataResolver for HttpClientMetadataResolver {
    async fn resolve(
        &self,
        client_id: &str,
    ) -> Result<Option<Client>, ClientMetadataResolverError> {
        let flight = match self.client_flight(client_id).await {
            Ok(flight) => flight,
            Err(failure) => return metadata_failure_result(client_id, failure),
        };
        flight
            .get_or_init(|| self.fetch_once(client_id))
            .await
            .clone()
    }
}

impl HttpClientMetadataResolver {
    /// 文書を取得して検査し、失敗箇所を呼び出し元で安全に分類できる形で返す。
    #[allow(clippy::type_complexity)]
    async fn fetch(
        &self,
        client_id: &str,
    ) -> Result<(Client, Option<Duration>), ClientMetadataFailure> {
        let url = url::Url::parse(client_id).map_err(|_| ClientMetadataFailure::ClientIdUrl)?;
        let host = url.host_str().ok_or(ClientMetadataFailure::ClientIdUrl)?;
        let port = url
            .port_or_known_default()
            .ok_or(ClientMetadataFailure::ClientIdUrl)?;
        let addresses = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| ClientMetadataFailure::DnsLookup)?
            .collect::<Vec<_>>();
        if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
            return Err(ClientMetadataFailure::NonPublicAddress);
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(self.timeout)
            .no_proxy()
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| ClientMetadataFailure::HttpClient)?;
        let mut response = client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| ClientMetadataFailure::HttpRequest)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(ClientMetadataFailure::HttpStatus(
                response.status().as_u16(),
            ));
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
            return Err(ClientMetadataFailure::ContentType);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_METADATA_BYTES as u64)
        {
            return Err(ClientMetadataFailure::ResponseTooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ClientMetadataFailure::ResponseBody)?
        {
            if body.len().saturating_add(chunk.len()) > MAX_METADATA_BYTES {
                return Err(ClientMetadataFailure::ResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let client = parse_client_metadata(client_id, &body)?;
        Ok((client, cache_lifetime))
    }
}

fn parse_client_metadata(client_id: &str, body: &[u8]) -> Result<Client, ClientMetadataFailure> {
    let document = serde_json::from_slice::<ClientMetadataDocument>(body)
        .map_err(|_| ClientMetadataFailure::DocumentFormat)?;
    if document.client_id != client_id {
        return Err(ClientMetadataFailure::ClientIdMismatch);
    }
    validate_public_client_authentication_methods(&document)?;
    if document
        .grant_types
        .as_ref()
        .is_some_and(|values| !values.iter().any(|value| value == "authorization_code"))
    {
        return Err(ClientMetadataFailure::GrantType);
    }
    if document
        .response_types
        .as_ref()
        .is_some_and(|values| !values.iter().any(|value| value == "code"))
    {
        return Err(ClientMetadataFailure::ResponseType);
    }
    Ok(Client {
        client_id: document.client_id,
        display_name: document.client_name,
        redirect_uris: document.redirect_uris,
    })
}

fn validate_public_client_authentication_methods(
    document: &ClientMetadataDocument,
) -> Result<(), ClientMetadataFailure> {
    if let (Some(selected), Some(supported)) = (
        document.token_endpoint_auth_method.as_deref(),
        document.token_endpoint_auth_methods_supported.as_ref(),
    ) && !supported.iter().any(|method| method == selected)
    {
        return Err(ClientMetadataFailure::AuthenticationMethodConflict);
    }
    if document
        .token_endpoint_auth_method
        .as_deref()
        .is_some_and(|method| {
            matches!(
                method,
                "client_secret_post" | "client_secret_basic" | "client_secret_jwt"
            )
        })
    {
        return Err(ClientMetadataFailure::AuthenticationMethodUnsupported);
    }
    let supports_none = document.token_endpoint_auth_method.as_deref() == Some("none")
        || document
            .token_endpoint_auth_methods_supported
            .as_ref()
            .is_some_and(|methods| methods.iter().any(|method| method == "none"));
    if !supports_none {
        return Err(ClientMetadataFailure::AuthenticationMethodUnsupported);
    }
    Ok(())
}

fn metadata_failure_result(client_id: &str, failure: ClientMetadataFailure) -> FetchResult {
    log_client_metadata_failure(client_id, failure);
    match failure.disposition() {
        ClientMetadataFailureDisposition::Rejected => Ok(None),
        ClientMetadataFailureDisposition::Unavailable
        | ClientMetadataFailureDisposition::Throttled => Err(ClientMetadataResolverError),
    }
}

fn log_client_metadata_failure(client_id: &str, failure: ClientMetadataFailure) {
    let client_host = url::Url::parse(client_id)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "<invalid>".into());
    match (failure.disposition(), failure) {
        (ClientMetadataFailureDisposition::Rejected, ClientMetadataFailure::HttpStatus(status)) => {
            tracing::warn!(
                event = "mcp.oauth.client_metadata.rejected",
                reason = failure.reason(),
                client_host,
                http_status = status,
                "MCP client metadata was rejected"
            );
        }
        (ClientMetadataFailureDisposition::Rejected, _) => {
            tracing::warn!(
                event = "mcp.oauth.client_metadata.rejected",
                reason = failure.reason(),
                client_host,
                "MCP client metadata was rejected"
            );
        }
        (
            ClientMetadataFailureDisposition::Unavailable,
            ClientMetadataFailure::HttpStatus(status),
        ) => {
            tracing::error!(
                event = "mcp.oauth.client_metadata.unavailable",
                reason = failure.reason(),
                client_host,
                http_status = status,
                "MCP client metadata resolution is unavailable"
            );
        }
        (ClientMetadataFailureDisposition::Unavailable, _) => {
            tracing::error!(
                event = "mcp.oauth.client_metadata.unavailable",
                reason = failure.reason(),
                client_host,
                "MCP client metadata resolution is unavailable"
            );
        }
        (ClientMetadataFailureDisposition::Throttled, _) => {
            tracing::warn!(
                event = "mcp.oauth.client_metadata.throttled",
                reason = failure.reason(),
                client_host,
                "MCP client metadata fetch was throttled"
            );
        }
    }
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
        || special_prefix(Ipv6Addr::UNSPECIFIED, 96)
        || special_prefix(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0, 0), 96)
        || special_prefix(Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0), 96)
        || special_prefix(Ipv6Addr::new(0x64, 0xff9b, 1, 0, 0, 0, 0, 0), 48)
        || special_prefix(Ipv6Addr::new(0x100, 0, 0, 0, 0, 0, 0, 0), 64)
        || special_prefix(Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23)
        || special_prefix(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0), 32)
        || special_prefix(Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16)
        || special_prefix(Ipv6Addr::new(0x2620, 0x4f, 0x8000, 0, 0, 0, 0, 0), 48)
        || special_prefix(Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20)
        || special_prefix(Ipv6Addr::new(0x5f00, 0, 0, 0, 0, 0, 0, 0), 16))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io, sync::Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

    impl CapturedLogs {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().expect("captured logs").clone()).expect("UTF-8 logs")
        }
    }

    impl io::Write for CapturedLogs {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .map_err(|_| io::Error::other("captured log lock was poisoned"))?
                .extend_from_slice(input);
            Ok(input.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> MakeWriter<'writer> for CapturedLogs {
        type Writer = Self;

        fn make_writer(&'writer self) -> Self::Writer {
            self.clone()
        }
    }

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
        let resolver = HttpClientMetadataResolver::new(Duration::from_secs(1));
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

    /// 同じclient IDの同時要求は、成功・失敗にかかわらず一つの取得結果を共有する。
    #[tokio::test]
    async fn concurrent_client_requests_share_one_flight() {
        let resolver = HttpClientMetadataResolver::new(Duration::from_secs(1));
        let client_id = "https://client.example/metadata.json";
        let first = resolver
            .client_flight(client_id)
            .await
            .expect("first flight");
        let second = resolver
            .client_flight(client_id)
            .await
            .expect("second flight");
        assert!(Arc::ptr_eq(&first, &second));
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let first_result = first.get_or_init(|| async {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(None)
        });
        let second_result = second.get_or_init(|| async {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(None)
        });
        let _ = tokio::join!(first_result, second_result);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn metadata_fetch_concurrency_is_bounded() {
        let resolver = HttpClientMetadataResolver::new(Duration::from_secs(1));
        let permits = (0..MAX_CONCURRENT_FETCHES)
            .map(|_| resolver.fetch_slots.try_acquire().expect("fetch slot"))
            .collect::<Vec<_>>();
        assert!(resolver.fetch_slots.try_acquire().is_err());
        drop(permits);
    }

    /// 取得できたclientはcacheから返し、取得の枠を消費しない。
    #[tokio::test]
    async fn cached_clients_do_not_consume_the_fetch_budget() {
        let resolver = HttpClientMetadataResolver::new(Duration::from_secs(1));
        let client_id = "https://client.example/metadata.json";
        let client = Client {
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
    fn client_metadata_accepts_single_and_multiple_public_client_methods() {
        let client_id = "https://client.example/oauth/metadata.json";
        let single = br#"{
            "client_id":"https://client.example/oauth/metadata.json",
            "client_name":"Example client",
            "redirect_uris":["http://127.0.0.1/callback"],
            "grant_types":["authorization_code"],
            "response_types":["code"],
            "token_endpoint_auth_method":"none"
        }"#;
        assert!(parse_client_metadata(client_id, single).is_ok());

        let multiple = br#"{
            "client_id":"https://client.example/oauth/metadata.json",
            "client_name":"ChatGPT",
            "redirect_uris":["https://chatgpt.com/connector/oauth/callback"],
            "grant_types":["authorization_code","refresh_token"],
            "response_types":["code"],
            "token_endpoint_auth_methods_supported":["none","private_key_jwt"]
        }"#;
        assert!(parse_client_metadata(client_id, multiple).is_ok());

        let preferred_private_key = br#"{
            "client_id":"https://client.example/oauth/metadata.json",
            "client_name":"Client with choices",
            "redirect_uris":["https://client.example/callback"],
            "token_endpoint_auth_method":"private_key_jwt",
            "token_endpoint_auth_methods_supported":["none","private_key_jwt"]
        }"#;
        assert!(parse_client_metadata(client_id, preferred_private_key).is_ok());
    }

    #[test]
    fn client_metadata_rejects_identity_and_authentication_method_errors() {
        let client_id = "https://client.example/oauth/metadata.json";
        let valid = br#"{
            "client_id":"https://client.example/oauth/metadata.json",
            "client_name":"Example client",
            "redirect_uris":["http://127.0.0.1/callback"],
            "token_endpoint_auth_method":"none"
        }"#;
        assert_eq!(
            parse_client_metadata("https://other.example/metadata.json", valid),
            Err(ClientMetadataFailure::ClientIdMismatch)
        );

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
        assert_eq!(
            parse_client_metadata(client_id, &confidential),
            Err(ClientMetadataFailure::AuthenticationMethodUnsupported)
        );

        let conflict = br#"{
            "client_id":"https://client.example/oauth/metadata.json",
            "client_name":"Conflicting client",
            "redirect_uris":["https://client.example/callback"],
            "token_endpoint_auth_method":"none",
            "token_endpoint_auth_methods_supported":["private_key_jwt"]
        }"#;
        assert_eq!(
            parse_client_metadata(client_id, conflict),
            Err(ClientMetadataFailure::AuthenticationMethodConflict)
        );

        let no_common_method = br#"{
            "client_id":"https://client.example/oauth/metadata.json",
            "client_name":"Private client",
            "redirect_uris":["https://client.example/callback"],
            "token_endpoint_auth_methods_supported":["private_key_jwt"]
        }"#;
        assert_eq!(
            parse_client_metadata(client_id, no_common_method),
            Err(ClientMetadataFailure::AuthenticationMethodUnsupported)
        );
    }

    #[test]
    fn client_metadata_failure_log_omits_the_client_id_path() {
        let logs = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_target(false)
            .compact()
            .with_writer(logs.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            log_client_metadata_failure(
                "https://client.example/private/metadata.json",
                ClientMetadataFailure::AuthenticationMethodUnsupported,
            );
            log_client_metadata_failure(
                "https://client.example/private/metadata.json",
                ClientMetadataFailure::HttpStatus(503),
            );
        });
        let output = logs.text();
        assert!(output.contains("mcp.oauth.client_metadata.rejected"));
        assert!(output.contains("mcp.oauth.client_metadata.unavailable"));
        assert!(output.contains("authentication-method-unsupported"));
        assert!(output.contains("http_status=503"));
        assert!(output.contains("client.example"));
        assert!(!output.contains("/private/"));
        assert!(!output.contains("metadata.json"));
    }

    #[test]
    fn client_metadata_http_status_distinguishes_rejection_from_unavailability() {
        assert_eq!(
            ClientMetadataFailure::HttpStatus(404).disposition(),
            ClientMetadataFailureDisposition::Rejected
        );
        assert_eq!(
            ClientMetadataFailure::HttpStatus(429).disposition(),
            ClientMetadataFailureDisposition::Unavailable
        );
        assert_eq!(
            ClientMetadataFailure::HttpStatus(503).disposition(),
            ClientMetadataFailureDisposition::Unavailable
        );
    }
}
