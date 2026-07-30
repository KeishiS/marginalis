//! 全HTTP responseに適用するbrowser security policy。

use axum::{
    http::{HeaderValue, header},
    middleware::Next,
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore as _, rngs::OsRng};

#[derive(Clone, Debug)]
pub(super) struct ContentSecurityPolicyNonce(String);

impl ContentSecurityPolicyNonce {
    pub(super) fn generate() -> Self {
        let mut bytes = [0_u8; 16];
        OsRng.fill_bytes(&mut bytes);
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(super) async fn security_headers(
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let content_security_policy = response
        .extensions()
        .get::<ContentSecurityPolicyNonce>()
        .map_or_else(
            || {
                HeaderValue::from_static(
                    "default-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
                )
            },
            |nonce| {
                // MathJaxのSVG出力は数式ごとの配置値をstyle属性へ設定する。style要素はnonceと
                // 同一originに限定したまま、属性だけを許可する。
                HeaderValue::from_str(&format!(
                    "default-src 'self'; style-src-elem 'self' 'nonce-{}'; style-src-attr 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
                    nonce.as_str()
                ))
                .expect("generated CSP nonce is a valid header value")
            },
        );
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_SECURITY_POLICY, content_security_policy);
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    // responseにはsession、CSRF token、OAuth codeまたはノート本文が含まれ得る。公開metadataも
    // 同じrouterを通るため、一律no-storeとして共有proxyの設定漏れを安全側に倒す。
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
}
