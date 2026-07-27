use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode, header},
};
use serde_json::{Value, json};
use tower::ServiceExt;

/// AxumのHTTP境界を通る、状態を持たない最小MCP Streamable HTTP client。
///
/// 外部client固有の挙動を再現せず、MCPとJSON-RPCの公開仕様だけを表現する。
pub struct McpTestClient<'a> {
    app: &'a Router,
    endpoint: &'a str,
    access_token: &'a str,
}

impl<'a> McpTestClient<'a> {
    pub fn new(app: &'a Router, endpoint: &'a str, access_token: &'a str) -> Self {
        Self {
            app,
            endpoint,
            access_token,
        }
    }

    pub async fn request(&self, id: u64, method: &str, params: Value) -> Response<Body> {
        self.post(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await
    }

    pub async fn notification(&self, method: &str) -> Response<Body> {
        self.post(json!({
            "jsonrpc": "2.0",
            "method": method,
        }))
        .await
    }

    pub async fn raw(&self, body: Value) -> Response<Body> {
        self.post(body).await
    }

    async fn post(&self, body: Value) -> Response<Body> {
        self.app
            .clone()
            .oneshot(
                Request::post(self.endpoint)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", self.access_token),
                    )
                    .body(Body::from(body.to_string()))
                    .expect("MCP request"),
            )
            .await
            .expect("MCP response")
    }
}

pub async fn json_response(response: Response<Body>) -> Value {
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("MCP response body");
    serde_json::from_slice(&body).expect("MCP JSON response")
}
