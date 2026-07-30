//! 利用者ごとのCSL-JSON書誌ライブラリー。

use std::sync::Arc;

use async_trait::async_trait;
use marginalis_domain::{Actor, BibliographyItem, BibliographyItemId, Revision, UnixMillis};
use serde_json::Value;

use crate::{Clock, Random};

const MAX_CSL_JSON_BYTES: usize = 131_072;
const MAX_CITATION_KEY_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BibliographyRepositoryError {
    NotFound,
    Conflict,
    CorruptData,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BibliographyUseCaseError {
    InvalidCslJson,
    NotFound,
    Conflict,
    Unavailable,
}

#[async_trait]
pub trait BibliographyRepository: Send + Sync {
    async fn search_owned_items(
        &self,
        actor: &Actor,
        query: &str,
    ) -> Result<Vec<BibliographyItem>, BibliographyRepositoryError>;

    async fn create_owned_item(
        &self,
        item: &BibliographyItem,
    ) -> Result<(), BibliographyRepositoryError>;

    #[allow(clippy::too_many_arguments)]
    async fn update_owned_item(
        &self,
        actor: &Actor,
        item_id: BibliographyItemId,
        citation_key: &str,
        csl_json: &str,
        updated_at: UnixMillis,
        expected_revision: Revision,
    ) -> Result<BibliographyItem, BibliographyRepositoryError>;

    async fn delete_owned_item(
        &self,
        actor: &Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
    ) -> Result<(), BibliographyRepositoryError>;
}

#[async_trait]
pub trait BibliographyUseCases: Send + Sync {
    async fn search_bibliography(
        &self,
        actor: Actor,
        query: String,
    ) -> Result<Vec<BibliographyItem>, BibliographyUseCaseError>;

    async fn add_bibliography_item(
        &self,
        actor: Actor,
        csl_json: Value,
    ) -> Result<BibliographyItem, BibliographyUseCaseError>;

    async fn update_bibliography_item(
        &self,
        actor: Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
        csl_json: Value,
    ) -> Result<BibliographyItem, BibliographyUseCaseError>;

    async fn delete_bibliography_item(
        &self,
        actor: Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
    ) -> Result<(), BibliographyUseCaseError>;
}

pub struct BibliographyApplication {
    repository: Arc<dyn BibliographyRepository>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
}

impl BibliographyApplication {
    pub fn new(
        repository: Arc<dyn BibliographyRepository>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
    ) -> Self {
        Self {
            repository,
            clock,
            random,
        }
    }
}

#[async_trait]
impl BibliographyUseCases for BibliographyApplication {
    async fn search_bibliography(
        &self,
        actor: Actor,
        query: String,
    ) -> Result<Vec<BibliographyItem>, BibliographyUseCaseError> {
        let query = query.trim();
        if query.len() > 256 || query.chars().any(char::is_control) {
            return Err(BibliographyUseCaseError::InvalidCslJson);
        }
        self.repository
            .search_owned_items(&actor, query)
            .await
            .map_err(map_repository_error)
    }

    async fn add_bibliography_item(
        &self,
        actor: Actor,
        csl_json: Value,
    ) -> Result<BibliographyItem, BibliographyUseCaseError> {
        let citation_key = validate_csl_json(&csl_json)?;
        let encoded = encode_csl_json(&csl_json)?;
        let item = BibliographyItem::create(
            BibliographyItemId::new(self.random.uuid_v7()),
            actor.identity(),
            citation_key,
            encoded,
            self.clock.now(),
        );
        self.repository
            .create_owned_item(&item)
            .await
            .map_err(map_repository_error)?;
        Ok(item)
    }

    async fn update_bibliography_item(
        &self,
        actor: Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
        csl_json: Value,
    ) -> Result<BibliographyItem, BibliographyUseCaseError> {
        let citation_key = validate_csl_json(&csl_json)?;
        let encoded = encode_csl_json(&csl_json)?;
        self.repository
            .update_owned_item(
                &actor,
                item_id,
                &citation_key,
                &encoded,
                self.clock.now(),
                expected_revision,
            )
            .await
            .map_err(map_repository_error)
    }

    async fn delete_bibliography_item(
        &self,
        actor: Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
    ) -> Result<(), BibliographyUseCaseError> {
        self.repository
            .delete_owned_item(&actor, item_id, expected_revision)
            .await
            .map_err(map_repository_error)
    }
}

fn validate_csl_json(value: &Value) -> Result<String, BibliographyUseCaseError> {
    let object = value
        .as_object()
        .ok_or(BibliographyUseCaseError::InvalidCslJson)?;
    let citation_key = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or(BibliographyUseCaseError::InvalidCslJson)?;
    let valid_key = !citation_key.is_empty()
        && citation_key.len() <= MAX_CITATION_KEY_BYTES
        && citation_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte));
    let valid_type = object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty() && value.len() <= 64);
    if !valid_key || !valid_type {
        return Err(BibliographyUseCaseError::InvalidCslJson);
    }
    Ok(citation_key.to_owned())
}

fn encode_csl_json(value: &Value) -> Result<String, BibliographyUseCaseError> {
    let encoded =
        serde_json::to_string(value).map_err(|_| BibliographyUseCaseError::InvalidCslJson)?;
    if encoded.len() > MAX_CSL_JSON_BYTES {
        return Err(BibliographyUseCaseError::InvalidCslJson);
    }
    Ok(encoded)
}

fn map_repository_error(error: BibliographyRepositoryError) -> BibliographyUseCaseError {
    match error {
        BibliographyRepositoryError::NotFound => BibliographyUseCaseError::NotFound,
        BibliographyRepositoryError::Conflict => BibliographyUseCaseError::Conflict,
        BibliographyRepositoryError::CorruptData | BibliographyRepositoryError::Unavailable => {
            BibliographyUseCaseError::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csl_json_requires_an_id_and_type() {
        assert_eq!(
            validate_csl_json(&serde_json::json!({
                "id": "smith2024",
                "type": "article-journal",
                "title": "Example"
            })),
            Ok("smith2024".into())
        );
        assert_eq!(
            validate_csl_json(&serde_json::json!({"id": "bad key", "type": "book"})),
            Err(BibliographyUseCaseError::InvalidCslJson)
        );
        assert_eq!(
            validate_csl_json(&serde_json::json!({"id": "smith2024"})),
            Err(BibliographyUseCaseError::InvalidCslJson)
        );
    }
}
