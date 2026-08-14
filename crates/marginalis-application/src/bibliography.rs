//! 利用者ごとのCSL-JSON文献ライブラリ。

use std::sync::Arc;

use async_trait::async_trait;
use marginalis_domain::{
    Actor, BibliographyItem, BibliographyItemId, Identity, Revision, UnixMillis, ValidatedCslJson,
};
use serde_json::Value;

use crate::{Clock, Random, StorageError};

/// 文献ライブラリ操作の失敗理由。
///
/// ここでの文言は開発者向けの記録用であり、利用者向けの`code`と`message`は
/// transport側の写像が決める。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BibliographyUseCaseError {
    #[error("bibliography search query is invalid")]
    InvalidSearchQuery,
    #[error("CSL-JSON input is invalid")]
    InvalidCslJson,
    #[error("bibliography item is not available")]
    NotFound,
    #[error("bibliography item conflicts")]
    Conflict,
    /// 一時的に処理できない。再試行で解消しうる。
    #[error("bibliography operation is unavailable")]
    Unavailable,
    /// 保存済みの内容が現行の規則を満たさない。再試行では解消しない。
    #[error("stored bibliography data is invalid")]
    CorruptData,
}

impl From<StorageError> for BibliographyUseCaseError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::NotFound => Self::NotFound,
            StorageError::Conflict => Self::Conflict,
            StorageError::CorruptData => Self::CorruptData,
            // 文献ライブラリに保存期限は無く、`RetentionExpired`はこの系統では発生しない。
            StorageError::RetentionExpired | StorageError::Unavailable => Self::Unavailable,
        }
    }
}

#[async_trait]
pub trait BibliographyRepository: Send + Sync {
    async fn search_owned_items(
        &self,
        actor: &Actor,
        query: &str,
    ) -> Result<Vec<BibliographyItem>, StorageError>;

    /// 指定した所有者のライブラリから、citation keyが一致する項目だけを読み取る。
    ///
    /// ノートの引用は作成者のライブラリで解決するため、閲覧している利用者ではなく
    /// 所有者のidentityを受け取る。呼び出し側は、閲覧できるノートの描画にだけ使う。
    async fn items_by_citation_keys(
        &self,
        owner: &Identity,
        citation_keys: &[String],
    ) -> Result<Vec<BibliographyItem>, StorageError>;

    async fn create_owned_item(&self, item: &BibliographyItem) -> Result<(), StorageError>;

    #[allow(clippy::too_many_arguments)]
    async fn update_owned_item(
        &self,
        actor: &Actor,
        item_id: BibliographyItemId,
        csl_json: &ValidatedCslJson,
        updated_at: UnixMillis,
        expected_revision: Revision,
    ) -> Result<BibliographyItem, StorageError>;

    async fn delete_owned_item(
        &self,
        actor: &Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
    ) -> Result<(), StorageError>;
}

/// transportへ公開する文献ライブラリ操作のapplication service。
///
/// 実装がこの1つだけでテストダブルも無いため、traitを介さず具体型のまま公開する。
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

    pub async fn search_bibliography(
        &self,
        actor: Actor,
        query: String,
    ) -> Result<Vec<BibliographyItem>, BibliographyUseCaseError> {
        let query = validate_search_query(&query)?;
        self.repository
            .search_owned_items(&actor, query)
            .await
            .map_err(BibliographyUseCaseError::from)
    }

    pub async fn add_bibliography_item(
        &self,
        actor: Actor,
        csl_json: Value,
    ) -> Result<BibliographyItem, BibliographyUseCaseError> {
        let validated = ValidatedCslJson::new(&csl_json)
            .map_err(|_| BibliographyUseCaseError::InvalidCslJson)?;
        let item = BibliographyItem::create(
            BibliographyItemId::new(self.random.uuid_v7()),
            actor.identity(),
            validated,
            self.clock.now(),
        );
        self.repository
            .create_owned_item(&item)
            .await
            .map_err(BibliographyUseCaseError::from)?;
        Ok(item)
    }

    pub async fn update_bibliography_item(
        &self,
        actor: Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
        csl_json: Value,
    ) -> Result<BibliographyItem, BibliographyUseCaseError> {
        let validated = ValidatedCslJson::new(&csl_json)
            .map_err(|_| BibliographyUseCaseError::InvalidCslJson)?;
        self.repository
            .update_owned_item(
                &actor,
                item_id,
                &validated,
                self.clock.now(),
                expected_revision,
            )
            .await
            .map_err(BibliographyUseCaseError::from)
    }

    pub async fn delete_bibliography_item(
        &self,
        actor: Actor,
        item_id: BibliographyItemId,
        expected_revision: Revision,
    ) -> Result<(), BibliographyUseCaseError> {
        self.repository
            .delete_owned_item(&actor, item_id, expected_revision)
            .await
            .map_err(BibliographyUseCaseError::from)
    }
}

fn validate_search_query(query: &str) -> Result<&str, BibliographyUseCaseError> {
    let query = query.trim();
    if query.chars().count() > 256 || query.chars().any(char::is_control) {
        return Err(BibliographyUseCaseError::InvalidSearchQuery);
    }
    Ok(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csl_json_requires_an_id_and_type() {
        assert_eq!(
            ValidatedCslJson::new(&serde_json::json!({
                "id": "smith2024",
                "type": "article-journal",
                "title": "Example"
            }))
            .map(|validated| validated.citation_key().to_owned()),
            Ok("smith2024".into())
        );
        assert!(
            ValidatedCslJson::new(&serde_json::json!({"id": "bad key", "type": "book"})).is_err()
        );
        assert!(ValidatedCslJson::new(&serde_json::json!({"id": "smith2024"})).is_err());
    }

    #[test]
    fn search_query_has_its_own_validation_error() {
        assert_eq!(validate_search_query("  smith  "), Ok("smith"));
        let maximum_japanese_query = "文".repeat(256);
        assert_eq!(
            validate_search_query(&maximum_japanese_query),
            Ok(maximum_japanese_query.as_str())
        );
        assert_eq!(
            validate_search_query(&"文".repeat(257)),
            Err(BibliographyUseCaseError::InvalidSearchQuery)
        );
        assert_eq!(
            validate_search_query("line\nbreak"),
            Err(BibliographyUseCaseError::InvalidSearchQuery)
        );
    }
}
