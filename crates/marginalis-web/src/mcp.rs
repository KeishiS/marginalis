//! MCP Streamable HTTPで使うJSON-RPC 2.0 wire型。

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct JsonRpcRequest {
    pub jsonrpc: Option<String>,
    #[serde(default)]
    pub id: JsonRpcId,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum JsonRpcId {
    #[default]
    Missing,
    Present(Value),
}

impl<'de> Deserialize<'de> for JsonRpcId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Value::deserialize(deserializer).map(Self::Present)
    }
}

impl JsonRpcId {
    pub(crate) fn is_notification(&self) -> bool {
        matches!(self, Self::Missing)
    }

    /// MCPはJSON-RPC 2.0より厳しく、request IDを文字列または整数に限定する。
    pub(crate) fn is_valid_mcp_id(&self) -> bool {
        match self {
            Self::Missing => true,
            Self::Present(Value::String(_)) => true,
            Self::Present(Value::Number(number)) => {
                number.as_i64().is_some() || number.as_u64().is_some()
            }
            Self::Present(_) => false,
        }
    }

    pub(crate) fn response_value(&self) -> Value {
        match self {
            Self::Missing => Value::Null,
            Self::Present(value) => value.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct JsonRpcError {
    pub code: i32,
    pub message: &'static str,
}

impl JsonRpcResponse {
    pub(crate) fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub(crate) fn error(id: Value, code: i32, message: &'static str) -> Self {
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
    fn request_requires_an_explicit_version_and_rejects_non_mcp_ids() {
        let missing_version: JsonRpcRequest =
            serde_json::from_str(r#"{"id":1,"method":"initialize"}"#).expect("request");
        assert_eq!(missing_version.jsonrpc, None);
        assert_eq!(missing_version.id, JsonRpcId::Present(serde_json::json!(1)));

        let explicit_null: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":null,"method":"initialize"}"#)
                .expect("request");
        assert!(!explicit_null.id.is_notification());
        assert!(!explicit_null.id.is_valid_mcp_id());

        let invalid_id: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":true,"method":"initialize"}"#)
                .expect("request");
        assert!(!invalid_id.id.is_valid_mcp_id());

        let fractional_id: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1.5,"method":"initialize"}"#)
                .expect("request");
        assert!(!fractional_id.id.is_valid_mcp_id());
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
