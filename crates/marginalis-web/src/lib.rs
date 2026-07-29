//! MarginalisのHTTP境界。
//!
//! 公開面は`/api/v3`、閲覧UI、OIDC、MCP Protected Resource Metadataおよび
//! Streamable HTTPだけである。

pub mod http;
mod mcp;

use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};

const REQUEST_ID_HEADER: &str = "x-request-id";

#[derive(Clone)]
struct RequestId(String);

/// 各requestへserver生成の相関IDを割り当て、response headerとtracing spanで共有する。
async fn assign_request_id(mut request: Request, next: Next) -> Response {
    let request_id = RequestId(uuid::Uuid::now_v7().to_string());
    request.extensions_mut().insert(request_id.clone());
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id.0) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}
