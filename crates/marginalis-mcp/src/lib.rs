//! MCPで使うJSON-RPC 2.0のtransport型。
//!
//! OAuth、認可、ノート操作はHTTP/application境界に置き、このcrateはwire formatだけを定義する。

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Clone, Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default = "json_rpc_version")]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

fn json_rpc_version() -> String {
    "2.0".into()
}

#[derive(Clone, Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Clone, Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: &'static str,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, code: i32, message: &'static str) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_defaults_to_json_rpc_two() {
        let request: JsonRpcRequest =
            serde_json::from_str(r#"{"id":1,"method":"initialize"}"#).expect("request");
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.id, Some(serde_json::json!(1)));
    }

    #[test]
    fn responses_expose_exactly_one_result_variant() {
        let success = serde_json::to_value(JsonRpcResponse::success(
            serde_json::json!(1),
            serde_json::json!({"ok": true}),
        ))
        .expect("success");
        assert!(success.get("result").is_some());
        assert!(success.get("error").is_none());

        let error = serde_json::to_value(JsonRpcResponse::error(
            serde_json::json!(1),
            -32600,
            "invalid request",
        ))
        .expect("error");
        assert!(error.get("result").is_none());
        assert_eq!(error["error"]["code"], -32600);
    }
}
