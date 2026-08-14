//! 文献情報の識別子と正本。

use core::fmt;
use serde_json::Value;

use super::{EntityId, Identity, Revision, UnixMillis};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BibliographyItemId(EntityId);

impl BibliographyItemId {
    pub const fn new(value: EntityId) -> Self {
        Self(value)
    }

    pub const fn entity_id(self) -> EntityId {
        self.0
    }
}

impl fmt::Display for BibliographyItemId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyItem {
    item_id: BibliographyItemId,
    owner: Identity,
    citation_key: String,
    csl_json: String,
    created_at: UnixMillis,
    updated_at: UnixMillis,
    revision: Revision,
}

const MAX_CSL_JSON_BYTES: usize = 131_072;
const MAX_CSL_STRING_BYTES: usize = 16_384;
const MAX_CSL_KEY_BYTES: usize = 128;
const MAX_CSL_DEPTH: usize = 32;
const MAX_CITATION_KEY_BYTES: usize = 128;

/// 保存可能な一項目分のCSL-JSON。
///
/// 生成時に構造と容量を検査するため、repositoryへ未検査のJSONを渡さない。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedCslJson {
    citation_key: String,
    encoded: String,
    value: Value,
}

impl ValidatedCslJson {
    pub fn new(value: &Value) -> Result<Self, InvalidBibliographyItem> {
        let object = value
            .as_object()
            .ok_or_else(|| InvalidBibliographyItem::new("item_not_object"))?;
        validate_json_value(value, 0)?;
        let citation_key = object
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| InvalidBibliographyItem::new("missing_id"))?;
        if !valid_identifier(citation_key, MAX_CITATION_KEY_BYTES) {
            return Err(InvalidBibliographyItem::new("invalid_id"));
        }
        let item_type = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| InvalidBibliographyItem::new("missing_type"))?;
        if item_type.trim().is_empty()
            || item_type.len() > 64
            || item_type.chars().any(char::is_control)
        {
            return Err(InvalidBibliographyItem::new("invalid_type"));
        }
        let encoded = serde_json::to_string(value)
            .map_err(|_| InvalidBibliographyItem::new("invalid_json"))?;
        if encoded.len() > MAX_CSL_JSON_BYTES {
            return Err(InvalidBibliographyItem::new("item_too_large"));
        }
        Ok(Self {
            citation_key: citation_key.to_owned(),
            encoded,
            value: value.clone(),
        })
    }

    pub fn from_encoded(
        citation_key: &str,
        encoded: &str,
    ) -> Result<Self, InvalidBibliographyItem> {
        let value = serde_json::from_str(encoded)
            .map_err(|_| InvalidBibliographyItem::new("invalid_json"))?;
        let validated = Self::new(&value)?;
        if validated.citation_key != citation_key {
            return Err(InvalidBibliographyItem::new("citation_key_mismatch"));
        }
        Ok(validated)
    }

    pub fn citation_key(&self) -> &str {
        &self.citation_key
    }

    pub fn encoded(&self) -> &str {
        &self.encoded
    }

    pub const fn value(&self) -> &Value {
        &self.value
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("bibliography item metadata is inconsistent: {code}")]
pub struct InvalidBibliographyItem {
    code: &'static str,
}

impl InvalidBibliographyItem {
    const fn new(code: &'static str) -> Self {
        Self { code }
    }

    pub const fn code(self) -> &'static str {
        self.code
    }
}

impl BibliographyItem {
    pub fn create(
        item_id: BibliographyItemId,
        owner: &Identity,
        csl_json: ValidatedCslJson,
        created_at: UnixMillis,
    ) -> Self {
        Self {
            item_id,
            owner: owner.clone(),
            citation_key: csl_json.citation_key,
            csl_json: csl_json.encoded,
            created_at,
            updated_at: created_at,
            revision: Revision::INITIAL,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        item_id: BibliographyItemId,
        owner: Identity,
        citation_key: String,
        csl_json: String,
        created_at: UnixMillis,
        updated_at: UnixMillis,
        revision: Revision,
    ) -> Result<Self, InvalidBibliographyItem> {
        if created_at > updated_at {
            return Err(InvalidBibliographyItem::new("invalid_timestamp_order"));
        }
        let csl_json = ValidatedCslJson::from_encoded(&citation_key, &csl_json)?;
        Ok(Self {
            item_id,
            owner,
            citation_key: csl_json.citation_key,
            csl_json: csl_json.encoded,
            created_at,
            updated_at,
            revision,
        })
    }

    pub const fn item_id(&self) -> BibliographyItemId {
        self.item_id
    }

    pub const fn owner(&self) -> &Identity {
        &self.owner
    }

    pub fn citation_key(&self) -> &str {
        &self.citation_key
    }

    pub fn csl_json(&self) -> &str {
        &self.csl_json
    }

    pub const fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    pub const fn updated_at(&self) -> UnixMillis {
        self.updated_at
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }
}

fn validate_json_value(value: &Value, depth: usize) -> Result<(), InvalidBibliographyItem> {
    if depth > MAX_CSL_DEPTH {
        return Err(InvalidBibliographyItem::new("item_too_deep"));
    }
    match value {
        Value::String(value) if value.len() > MAX_CSL_STRING_BYTES => {
            return Err(InvalidBibliographyItem::new("string_too_long"));
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
                    return Err(InvalidBibliographyItem::new("invalid_object_key"));
                }
                validate_json_value(value, depth + 1)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
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
    fn restored_item_requires_valid_matching_csl_json() {
        let owner = Identity::new("https://issuer.example".into(), "alice".into()).unwrap();
        let item_id =
            BibliographyItemId::new("0197c9bc-0000-7000-8000-000000000001".parse().unwrap());
        let restore = |citation_key: &str, csl_json: &str| {
            BibliographyItem::restore(
                item_id,
                owner.clone(),
                citation_key.into(),
                csl_json.into(),
                UnixMillis::new(1),
                UnixMillis::new(1),
                Revision::INITIAL,
            )
        };

        assert!(restore("smith", r#"{"id":"smith","type":"book"}"#).is_ok());
        assert!(restore("smith", r#"[]"#).is_err());
        assert!(restore("smith", r#"{"id":"jones","type":"book"}"#).is_err());
        assert!(restore("smith", r#"{"id":"smith","type":""}"#).is_err());
        assert!(restore("smith", r#"{"id":"smith","type":"  "}"#).is_err());
    }

    #[test]
    fn csl_json_validation_enforces_resource_limits() {
        assert!(
            ValidatedCslJson::new(&serde_json::json!({
                "id": "item", "type": "book", "title": "x".repeat(MAX_CSL_STRING_BYTES + 1)
            }))
            .is_err()
        );

        let mut long_key = serde_json::Map::new();
        long_key.insert("id".into(), Value::String("item".into()));
        long_key.insert("type".into(), Value::String("book".into()));
        long_key.insert("x".repeat(MAX_CSL_KEY_BYTES + 1), Value::Null);
        assert!(ValidatedCslJson::new(&Value::Object(long_key)).is_err());

        let mut deep = Value::Null;
        for _ in 0..=MAX_CSL_DEPTH {
            deep = Value::Array(vec![deep]);
        }
        assert!(
            ValidatedCslJson::new(
                &serde_json::json!({"id": "item", "type": "book", "value": deep}),
            )
            .is_err()
        );

        assert!(
            ValidatedCslJson::new(&serde_json::json!({
                "id": "item", "type": "book", "value": vec![Value::Null; MAX_CSL_JSON_BYTES]
            }))
            .is_err()
        );
    }
}
