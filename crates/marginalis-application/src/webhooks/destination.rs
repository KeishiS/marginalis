//! Webhook送信先URLの検査規則。

/// 検査済みの送信先。adapterはこの結果のhostとportで接続する。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookDestination {
    pub host: String,
    pub port: u16,
    /// 管理者allowlistに含まれ、address検査とHTTPS限定を免除するか。
    pub exempt: bool,
    /// hostがIPのliteralの場合、そのaddress。
    pub literal: Option<std::net::IpAddr>,
}

/// 送信先URLが検査を通らなかったことを示す。理由は応答へ出さない。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("webhook destination URL is not allowed")]
pub struct InvalidWebhookDestination;

/// 送信先URLの検査。public HTTPS(port 443)以外とuserinfo付きURLを拒否し、
/// IPのliteralは特別用途addressを拒否する。allowlistのhostだけを例外にする。
/// 名前で指定された宛先の解決結果の検査は、送信直前にadapterが行う。
pub fn validate_webhook_destination(
    url: &str,
    allowed_hosts: &[String],
) -> Result<WebhookDestination, InvalidWebhookDestination> {
    let parsed = url::Url::parse(url).map_err(|_| InvalidWebhookDestination)?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(InvalidWebhookDestination);
    }
    let Some(host) = parsed.host_str().map(str::to_string) else {
        return Err(InvalidWebhookDestination);
    };
    let exempt = allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&host));
    if !exempt {
        if parsed.scheme() != "https" {
            return Err(InvalidWebhookDestination);
        }
        if parsed.port().is_some_and(|port| port != 443) {
            return Err(InvalidWebhookDestination);
        }
    }
    let port = parsed
        .port_or_known_default()
        .ok_or(InvalidWebhookDestination)?;
    let literal = match parsed.host() {
        Some(url::Host::Ipv4(v4)) => Some(std::net::IpAddr::V4(v4)),
        Some(url::Host::Ipv6(v6)) => Some(std::net::IpAddr::V6(v6)),
        _ => None,
    };
    if !exempt && literal.is_some_and(|address| !is_public_webhook_address(address)) {
        return Err(InvalidWebhookDestination);
    }
    Ok(WebhookDestination {
        host,
        port,
        exempt,
        literal,
    })
}

/// 公開networkのaddressだけを許す。特別用途address(RFC 6890)を拒否する。
pub fn is_public_webhook_address(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(v4) => {
            let octets = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                // 100.64.0.0/10 (shared address space)
                || (octets[0] == 100 && (octets[1] & 0b1100_0000) == 64)
                // 192.0.0.0/24 (IETF protocol assignments)
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                // 198.18.0.0/15 (benchmarking)
                || (octets[0] == 198 && (octets[1] & 0b1111_1110) == 18)
                // 240.0.0.0/4 (reserved)
                || octets[0] >= 240)
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_webhook_address(std::net::IpAddr::V4(mapped));
            }
            let segments = v6.segments();
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                // fc00::/7 (unique local)
                || (segments[0] & 0xfe00) == 0xfc00
                // fe80::/10 (link local)
                || (segments[0] & 0xffc0) == 0xfe80
                // 2001:db8::/32 (documentation)
                || (segments[0] == 0x2001 && segments[1] == 0x0db8))
        }
    }
}
