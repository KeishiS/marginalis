//! 書誌ライブラリーへ保存するCSL-JSON一項目の検査と正規化。

use serde_json::Value;

const MAX_CSL_JSON_BYTES: usize = 131_072;
const MAX_CSL_STRING_BYTES: usize = 16_384;
const MAX_CSL_KEY_BYTES: usize = 128;
const MAX_CSL_DEPTH: usize = 32;
const MAX_CITATION_KEY_BYTES: usize = 128;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ValidatedCslJson {
    pub(crate) citation_key: String,
    pub(crate) encoded: String,
}

pub(crate) fn validate_and_encode(value: &Value) -> Result<ValidatedCslJson, &'static str> {
    let object = value.as_object().ok_or("item_not_object")?;
    validate_json_value(value, 0)?;
    let citation_key = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or("missing_id")?;
    if !valid_identifier(citation_key, MAX_CITATION_KEY_BYTES) {
        return Err("invalid_id");
    }
    let item_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or("missing_type")?;
    if item_type.is_empty() || item_type.len() > 64 || item_type.chars().any(char::is_control) {
        return Err("invalid_type");
    }
    let encoded = serde_json::to_string(value).map_err(|_| "invalid_json")?;
    if encoded.len() > MAX_CSL_JSON_BYTES {
        return Err("item_too_large");
    }
    Ok(ValidatedCslJson {
        citation_key: citation_key.to_owned(),
        encoded,
    })
}

fn validate_json_value(value: &Value, depth: usize) -> Result<(), &'static str> {
    if depth > MAX_CSL_DEPTH {
        return Err("item_too_deep");
    }
    match value {
        Value::String(value) => {
            if value.len() > MAX_CSL_STRING_BYTES {
                return Err("string_too_long");
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_json_value(value, depth + 1)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if key.is_empty()
                    || key.len() > MAX_CSL_KEY_BYTES
                    || key.chars().any(char::is_control)
                {
                    return Err("invalid_object_key");
                }
                validate_json_value(value, depth + 1)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn valid_identifier(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_enforces_string_key_depth_and_encoded_size_limits() {
        let long_string = serde_json::json!({
            "id": "item", "type": "book", "title": "x".repeat(MAX_CSL_STRING_BYTES + 1)
        });
        assert_eq!(
            validate_and_encode(&long_string).unwrap_err(),
            "string_too_long"
        );

        let mut long_key = serde_json::Map::new();
        long_key.insert("id".into(), Value::String("item".into()));
        long_key.insert("type".into(), Value::String("book".into()));
        long_key.insert("x".repeat(MAX_CSL_KEY_BYTES + 1), Value::Null);
        assert_eq!(
            validate_and_encode(&Value::Object(long_key)).unwrap_err(),
            "invalid_object_key"
        );

        let mut deep = Value::Null;
        for _ in 0..=MAX_CSL_DEPTH {
            deep = Value::Array(vec![deep]);
        }
        let deep = serde_json::json!({"id": "item", "type": "book", "value": deep});
        assert_eq!(validate_and_encode(&deep).unwrap_err(), "item_too_deep");

        let large_array = Value::Array(vec![Value::Null; MAX_CSL_JSON_BYTES]);
        let too_large = serde_json::json!({
            "id": "item", "type": "book", "value": large_array
        });
        assert_eq!(
            validate_and_encode(&too_large).unwrap_err(),
            "item_too_large"
        );
    }
}
