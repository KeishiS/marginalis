//! CSL-JSONファイルの事前確認付き一方向取り込み。

use std::sync::Arc;

use async_trait::async_trait;
use marginalis_domain::{
    Actor, BibliographyContentDigest, BibliographyImportLink, BibliographyImportSource,
    BibliographyImportSourceId, BibliographyItem, BibliographyItemId,
    MAX_BIBLIOGRAPHY_IMPORT_SOURCE_NAME_CHARACTERS, Revision, UnixMillis,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{Clock, Random, csl_json::validate_and_encode};

mod plan;
mod preview;

use plan::build_commit;
#[cfg(test)]
use plan::can_create_separate;
#[cfg(test)]
use preview::import_state_token;
use preview::{classify_import, is_canonical_digest};

#[derive(Debug)]
struct ValidatedItem {
    external_item_id: String,
    citation_key: String,
    encoded: String,
    digest: BibliographyContentDigest,
}

fn validate_item(value: &Value) -> Result<ValidatedItem, &'static str> {
    let validated = validate_and_encode(value)?;
    let digest =
        BibliographyContentDigest::new(Sha256::digest(validated.encoded.as_bytes()).into());
    Ok(ValidatedItem {
        external_item_id: validated.citation_key.clone(),
        citation_key: validated.citation_key,
        encoded: validated.encoded,
        digest,
    })
}

pub const MAX_IMPORT_ITEMS: usize = 1_000;
pub const MAX_IMPORT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BibliographyImportSourceSelection {
    New {
        display_name: String,
    },
    Existing {
        source_id: BibliographyImportSourceId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyImportInput {
    pub source: BibliographyImportSourceSelection,
    pub items: Vec<Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BibliographyImportClassification {
    Create,
    UpdateFromExternal,
    Unchanged,
    KeepLocal,
    Conflict,
    DuplicateCandidate,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyImportCandidate {
    pub item_id: BibliographyItemId,
    pub citation_key: String,
    pub title: Option<String>,
    pub revision: Revision,
    pub matched_by: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyImportEntry {
    /// CSL-JSON配列内の0から始まる位置。
    pub position: usize,
    pub external_item_id: Option<String>,
    pub citation_key: Option<String>,
    pub classification: BibliographyImportClassification,
    pub item_id: Option<BibliographyItemId>,
    pub item_revision: Option<Revision>,
    /// 対応済み項目について、事前確認時に読み取ったMarginalis側のCSL-JSON。
    pub current_csl_json: Option<Value>,
    pub candidates: Vec<BibliographyImportCandidate>,
    pub rejection_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyImportPreview {
    pub source_id: Option<BibliographyImportSourceId>,
    pub source_revision: Option<Revision>,
    pub preview_token: String,
    pub entries: Vec<BibliographyImportEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BibliographyImportDecisionKind {
    ApplySuggested,
    CreateSeparate,
    KeepLocal,
    UseExternal,
    LinkExistingKeepLocal,
    LinkExistingUseExternal,
    Exclude,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyImportDecision {
    pub position: usize,
    pub kind: BibliographyImportDecisionKind,
    pub candidate_item_id: Option<BibliographyItemId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyImportState {
    pub source: Option<BibliographyImportSource>,
    pub links: Vec<BibliographyImportLink>,
    pub items: Vec<BibliographyItem>,
}

impl BibliographyImportState {
    /// adapterごとの取得順に依存せず、事前確認状態を比較できる順序へそろえる。
    pub fn canonicalized(mut self) -> Self {
        self.links.sort_by(|left, right| {
            (left.source_id().to_string(), left.external_item_id())
                .cmp(&(right.source_id().to_string(), right.external_item_id()))
        });
        self.items.sort_by_key(|item| item.item_id().to_string());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BibliographyImportItemMutation {
    Create {
        item: BibliographyItem,
        link: BibliographyImportLink,
    },
    Update {
        item_id: BibliographyItemId,
        csl_json: String,
        expected_revision: Revision,
        link: BibliographyImportLink,
        updated_at: UnixMillis,
    },
    Keep {
        expected_revision: Revision,
        link: BibliographyImportLink,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyImportCommit {
    pub source: BibliographyImportSource,
    /// 事前確認時に読み取った永続化状態。adapterは保存と同じtransaction内で再照合する。
    pub expected_state: BibliographyImportState,
    pub imported_at: UnixMillis,
    pub mutations: Vec<BibliographyImportItemMutation>,
    pub excluded: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyImportResult {
    pub source_id: BibliographyImportSourceId,
    pub source_revision: Revision,
    pub created: usize,
    pub updated: usize,
    pub kept: usize,
    pub excluded: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BibliographyImportRepositoryError {
    #[error("bibliography import source was not found")]
    NotFound,
    #[error("bibliography import state changed")]
    Conflict,
    #[error("stored bibliography import data is invalid")]
    CorruptData,
    #[error("bibliography import storage is unavailable")]
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BibliographyImportUseCaseError {
    #[error("bibliography import input is invalid: {0}")]
    InvalidInput(&'static str),
    #[error("bibliography import decisions are incomplete or invalid")]
    InvalidDecision,
    #[error("bibliography import source was not found")]
    NotFound,
    #[error("bibliography import must be previewed again")]
    Conflict,
    #[error("stored bibliography import data is invalid")]
    CorruptData,
    #[error("bibliography import is unavailable")]
    Unavailable,
}

#[async_trait]
pub trait BibliographyImportRepository: Send + Sync {
    async fn list_import_sources(
        &self,
        actor: &Actor,
    ) -> Result<Vec<BibliographyImportSource>, BibliographyImportRepositoryError>;

    async fn load_import_state(
        &self,
        actor: &Actor,
        source_id: Option<BibliographyImportSourceId>,
    ) -> Result<BibliographyImportState, BibliographyImportRepositoryError>;

    async fn apply_import(
        &self,
        actor: &Actor,
        commit: BibliographyImportCommit,
    ) -> Result<BibliographyImportResult, BibliographyImportRepositoryError>;
}

#[async_trait]
pub trait BibliographyImportUseCases: Send + Sync {
    async fn list_bibliography_import_sources(
        &self,
        actor: Actor,
    ) -> Result<Vec<BibliographyImportSource>, BibliographyImportUseCaseError>;

    async fn preview_bibliography_import(
        &self,
        actor: Actor,
        input: BibliographyImportInput,
    ) -> Result<BibliographyImportPreview, BibliographyImportUseCaseError>;

    async fn apply_bibliography_import(
        &self,
        actor: Actor,
        input: BibliographyImportInput,
        decisions: Vec<BibliographyImportDecision>,
        preview_token: String,
    ) -> Result<BibliographyImportResult, BibliographyImportUseCaseError>;
}

pub struct BibliographyImportApplication {
    repository: Arc<dyn BibliographyImportRepository>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn Random>,
}

impl BibliographyImportApplication {
    pub fn new(
        repository: Arc<dyn BibliographyImportRepository>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn Random>,
    ) -> Self {
        Self {
            repository,
            clock,
            random,
        }
    }

    async fn preview_with_state(
        &self,
        actor: &Actor,
        input: &BibliographyImportInput,
    ) -> Result<(BibliographyImportPreview, BibliographyImportState), BibliographyImportUseCaseError>
    {
        validate_import_input(input)?;
        let source_id = match input.source {
            BibliographyImportSourceSelection::New { .. } => None,
            BibliographyImportSourceSelection::Existing { source_id } => Some(source_id),
        };
        let state = self
            .repository
            .load_import_state(actor, source_id)
            .await
            .map_err(map_repository_error)?
            .canonicalized();
        if source_id.is_some() && state.source.is_none() {
            return Err(BibliographyImportUseCaseError::NotFound);
        }
        let preview = classify_import(input, &state)?;
        Ok((preview, state))
    }
}

#[async_trait]
impl BibliographyImportUseCases for BibliographyImportApplication {
    async fn list_bibliography_import_sources(
        &self,
        actor: Actor,
    ) -> Result<Vec<BibliographyImportSource>, BibliographyImportUseCaseError> {
        self.repository
            .list_import_sources(&actor)
            .await
            .map_err(map_repository_error)
    }

    async fn preview_bibliography_import(
        &self,
        actor: Actor,
        input: BibliographyImportInput,
    ) -> Result<BibliographyImportPreview, BibliographyImportUseCaseError> {
        self.preview_with_state(&actor, &input)
            .await
            .map(|(preview, _)| preview)
    }

    async fn apply_bibliography_import(
        &self,
        actor: Actor,
        input: BibliographyImportInput,
        decisions: Vec<BibliographyImportDecision>,
        preview_token: String,
    ) -> Result<BibliographyImportResult, BibliographyImportUseCaseError> {
        let (preview, state) = self.preview_with_state(&actor, &input).await?;
        if !is_canonical_digest(&preview_token) {
            return Err(BibliographyImportUseCaseError::InvalidInput(
                "invalid_preview_token",
            ));
        }
        if preview.preview_token != preview_token {
            return Err(BibliographyImportUseCaseError::Conflict);
        }
        let imported_at = self.clock.now();
        let source = match (&input.source, state.source.as_ref()) {
            (BibliographyImportSourceSelection::New { display_name }, None) => {
                BibliographyImportSource::create(
                    BibliographyImportSourceId::new(self.random.uuid_v7()),
                    actor.identity(),
                    display_name.trim().to_owned(),
                    imported_at,
                )
                .map_err(|_| BibliographyImportUseCaseError::InvalidInput("invalid_source_name"))?
            }
            (BibliographyImportSourceSelection::Existing { .. }, Some(source)) => source.clone(),
            _ => return Err(BibliographyImportUseCaseError::Conflict),
        };
        let commit = build_commit(
            &input,
            &preview,
            state,
            source,
            decisions,
            imported_at,
            self.random.as_ref(),
        )?;
        self.repository
            .apply_import(&actor, commit)
            .await
            .map_err(map_repository_error)
    }
}

fn validate_import_input(
    input: &BibliographyImportInput,
) -> Result<(), BibliographyImportUseCaseError> {
    if input.items.is_empty() || input.items.len() > MAX_IMPORT_ITEMS {
        return Err(BibliographyImportUseCaseError::InvalidInput(
            "invalid_item_count",
        ));
    }
    if let BibliographyImportSourceSelection::New { display_name } = &input.source {
        let trimmed = display_name.trim();
        if trimmed.is_empty()
            || trimmed.chars().count() > MAX_BIBLIOGRAPHY_IMPORT_SOURCE_NAME_CHARACTERS
            || trimmed.chars().any(char::is_control)
        {
            return Err(BibliographyImportUseCaseError::InvalidInput(
                "invalid_source_name",
            ));
        }
    }
    let encoded = serde_json::to_vec(&input.items)
        .map_err(|_| BibliographyImportUseCaseError::InvalidInput("invalid_json"))?;
    if encoded.len() > MAX_IMPORT_BYTES {
        return Err(BibliographyImportUseCaseError::InvalidInput(
            "input_too_large",
        ));
    }
    Ok(())
}

fn map_repository_error(
    error: BibliographyImportRepositoryError,
) -> BibliographyImportUseCaseError {
    match error {
        BibliographyImportRepositoryError::NotFound => BibliographyImportUseCaseError::NotFound,
        BibliographyImportRepositoryError::Conflict => BibliographyImportUseCaseError::Conflict,
        BibliographyImportRepositoryError::CorruptData => {
            BibliographyImportUseCaseError::CorruptData
        }
        BibliographyImportRepositoryError::Unavailable => {
            BibliographyImportUseCaseError::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{BibliographyContentDigest, EntityId, Identity};

    use super::*;

    fn source_id() -> BibliographyImportSourceId {
        BibliographyImportSourceId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-0000000000b1").expect("UUIDv7"),
        )
    }

    fn item_id() -> BibliographyItemId {
        BibliographyItemId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-0000000000b2").expect("UUIDv7"),
        )
    }

    fn owner() -> Identity {
        Identity::new("https://id.example.test".into(), "alice".into()).expect("identity")
    }

    fn source() -> BibliographyImportSource {
        BibliographyImportSource::create(
            source_id(),
            &owner(),
            "Zotero".into(),
            UnixMillis::new(10),
        )
        .expect("source")
    }

    fn item(revision: Revision) -> BibliographyItem {
        BibliographyItem::restore(
            item_id(),
            owner(),
            "smith2024".into(),
            r#"{"DOI":"10.1/example","id":"smith2024","title":"Example","type":"article-journal"}"#
                .into(),
            UnixMillis::new(10),
            UnixMillis::new(10),
            revision,
        )
        .expect("item")
    }

    fn input(value: Value) -> BibliographyImportInput {
        BibliographyImportInput {
            source: BibliographyImportSourceSelection::Existing {
                source_id: source_id(),
            },
            items: vec![value],
        }
    }

    #[test]
    fn linked_item_is_classified_from_both_baselines() {
        let value = serde_json::json!({
            "DOI": "10.1/example", "id": "smith2024", "title": "Example", "type": "article-journal"
        });
        let validated = validate_item(&value).expect("valid input");
        for (revision, digest, expected) in [
            (
                Revision::INITIAL,
                validated.digest,
                BibliographyImportClassification::Unchanged,
            ),
            (
                Revision::new(2).expect("revision"),
                validated.digest,
                BibliographyImportClassification::KeepLocal,
            ),
            (
                Revision::INITIAL,
                BibliographyContentDigest::new([0; 32]),
                BibliographyImportClassification::UpdateFromExternal,
            ),
            (
                Revision::new(2).expect("revision"),
                BibliographyContentDigest::new([0; 32]),
                BibliographyImportClassification::Conflict,
            ),
        ] {
            let state = BibliographyImportState {
                source: Some(source()),
                links: vec![
                    BibliographyImportLink::new(
                        source_id(),
                        "smith2024".into(),
                        item_id(),
                        digest,
                        Revision::INITIAL,
                    )
                    .expect("link"),
                ],
                items: vec![item(revision)],
            };
            let preview = classify_import(&input(value.clone()), &state).expect("preview");
            assert_eq!(preview.entries[0].classification, expected);
            assert_eq!(
                preview.entries[0]
                    .current_csl_json
                    .as_ref()
                    .and_then(|value| value["title"].as_str()),
                Some("Example")
            );
        }
    }

    #[test]
    fn unlinked_identifier_match_is_only_a_candidate() {
        let state = BibliographyImportState {
            source: Some(source()),
            links: Vec::new(),
            items: vec![item(Revision::INITIAL)],
        };
        let preview = classify_import(
            &input(serde_json::json!({
                "DOI": "10.1/EXAMPLE", "id": "other", "title": "Other", "type": "article-journal"
            })),
            &state,
        )
        .expect("preview");
        assert_eq!(
            preview.entries[0].classification,
            BibliographyImportClassification::DuplicateCandidate
        );
        assert_eq!(preview.entries[0].candidates[0].matched_by, vec!["doi"]);

        let blank_values = BibliographyImportState {
            source: Some(source()),
            links: Vec::new(),
            items: vec![
                BibliographyItem::restore(
                    item_id(),
                    owner(),
                    "stored-blank".into(),
                    r#"{"DOI":" ","id":"stored-blank","title":"","type":"book"}"#.into(),
                    UnixMillis::new(10),
                    UnixMillis::new(10),
                    Revision::INITIAL,
                )
                .expect("item"),
            ],
        };
        let preview = classify_import(
            &input(serde_json::json!({
                "DOI": "", "id": "new", "title": " ", "type": "book"
            })),
            &blank_values,
        )
        .expect("preview");
        assert_eq!(
            preview.entries[0].classification,
            BibliographyImportClassification::Create
        );
    }

    #[test]
    fn invalid_positions_are_reported_without_rejecting_the_preview() {
        let state = BibliographyImportState {
            source: Some(source()),
            links: Vec::new(),
            items: Vec::new(),
        };
        let preview = classify_import(
            &BibliographyImportInput {
                source: BibliographyImportSourceSelection::Existing {
                    source_id: source_id(),
                },
                items: vec![
                    serde_json::json!({"type": "book"}),
                    serde_json::json!({"id": "ok", "type": "book"}),
                    serde_json::json!("not-an-item"),
                    serde_json::json!({"id": "ok", "type": "article"}),
                ],
            },
            &state,
        )
        .expect("preview");
        assert_eq!(preview.entries[0].position, 0);
        assert_eq!(
            preview.entries[0].rejection_code.as_deref(),
            Some("missing_id")
        );
        assert_eq!(
            preview.entries[1].classification,
            BibliographyImportClassification::Create
        );
        assert_eq!(
            preview.entries[2].rejection_code.as_deref(),
            Some("item_not_object")
        );
        assert_eq!(
            preview.entries[3].rejection_code.as_deref(),
            Some("duplicate_external_item_id")
        );
    }

    #[test]
    fn preview_token_binds_the_input_and_every_bibliography_revision() {
        let value = serde_json::json!({
            "id": "smith2024", "title": "Example", "type": "article-journal"
        });
        let original_input = input(value.clone());
        let original_state = BibliographyImportState {
            source: Some(source()),
            links: Vec::new(),
            items: vec![item(Revision::INITIAL)],
        };
        let token = import_state_token(&original_input, &original_state).expect("token");
        assert!(is_canonical_digest(&token));
        assert_eq!(
            import_state_token(&original_input, &original_state).expect("same token"),
            token
        );

        let changed_input = input(serde_json::json!({
            "id": "smith2024", "title": "Changed", "type": "article-journal"
        }));
        assert_ne!(
            import_state_token(&changed_input, &original_state).expect("input token"),
            token
        );
        let changed_state = BibliographyImportState {
            items: vec![item(Revision::new(2).expect("revision"))],
            ..original_state
        };
        assert_ne!(
            import_state_token(&original_input, &changed_state).expect("state token"),
            token
        );
    }

    #[test]
    fn a_duplicate_citation_key_cannot_be_created_separately() {
        let state = BibliographyImportState {
            source: Some(source()),
            links: Vec::new(),
            items: vec![item(Revision::INITIAL)],
        };
        let preview = classify_import(
            &input(serde_json::json!({
                "id": "smith2024", "title": "Different", "type": "book"
            })),
            &state,
        )
        .expect("preview");
        assert_eq!(
            preview.entries[0].classification,
            BibliographyImportClassification::DuplicateCandidate
        );
        assert!(!can_create_separate(&preview.entries[0]));
    }

    #[test]
    fn import_input_enforces_file_level_resource_limits() {
        let too_many = BibliographyImportInput {
            source: BibliographyImportSourceSelection::New {
                display_name: "Zotero".into(),
            },
            items: (0..=MAX_IMPORT_ITEMS)
                .map(|position| serde_json::json!({"id": position, "type": "book"}))
                .collect(),
        };
        assert_eq!(
            validate_import_input(&too_many),
            Err(BibliographyImportUseCaseError::InvalidInput(
                "invalid_item_count"
            ))
        );

        let too_large = BibliographyImportInput {
            source: BibliographyImportSourceSelection::New {
                display_name: "Zotero".into(),
            },
            items: (0..70)
                .map(|position| {
                    serde_json::json!({
                        "id": format!("item-{position}"),
                        "title": "x".repeat(125_000),
                        "type": "book"
                    })
                })
                .collect(),
        };
        assert_eq!(
            validate_import_input(&too_large),
            Err(BibliographyImportUseCaseError::InvalidInput(
                "input_too_large"
            ))
        );
    }

    #[test]
    fn import_source_name_is_trimmed_but_rejects_controls_and_excess_length() {
        for display_name in [
            "Zo\ttero",
            &"x".repeat(MAX_BIBLIOGRAPHY_IMPORT_SOURCE_NAME_CHARACTERS + 1),
        ] {
            let input = BibliographyImportInput {
                source: BibliographyImportSourceSelection::New {
                    display_name: display_name.into(),
                },
                items: vec![serde_json::json!({"id": "item", "type": "book"})],
            };
            assert_eq!(
                validate_import_input(&input),
                Err(BibliographyImportUseCaseError::InvalidInput(
                    "invalid_source_name"
                ))
            );
        }

        let valid = BibliographyImportInput {
            source: BibliographyImportSourceSelection::New {
                display_name: "  Zotero  ".into(),
            },
            items: vec![serde_json::json!({"id": "item", "type": "book"})],
        };
        assert_eq!(validate_import_input(&valid), Ok(()));
    }
}
