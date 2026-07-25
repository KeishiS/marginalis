//! 全HTTP responseに適用するbrowser security policy。

use axum::{
    http::{HeaderValue, header},
    middleware::Next,
    response::Response,
};

pub(super) async fn security_headers(
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    );
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
