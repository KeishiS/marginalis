//! 外部書誌情報の一方向取り込みで永続化する業務モデル。

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{BibliographyItemId, EntityId, Identity, Revision, UnixMillis};

pub const MAX_BIBLIOGRAPHY_IMPORT_SOURCE_NAME_CHARACTERS: usize = 128;
pub const MAX_BIBLIOGRAPHY_EXTERNAL_ITEM_ID_BYTES: usize = 128;

/// 利用者が登録した取込元の内部識別子。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BibliographyImportSourceId(EntityId);

impl BibliographyImportSourceId {
    pub const fn new(value: EntityId) -> Self {
        Self(value)
    }

    pub const fn entity_id(self) -> EntityId {
        self.0
    }
}

impl fmt::Display for BibliographyImportSourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// 取込元から書誌情報を受け取る方式。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BibliographyImportMethod {
    CslJsonFile,
}

/// 利用者が再取り込み時に選ぶ取込元。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyImportSource {
    source_id: BibliographyImportSourceId,
    owner: Identity,
    method: BibliographyImportMethod,
    display_name: String,
    revision: Revision,
    created_at: UnixMillis,
    last_imported_at: UnixMillis,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("bibliography import source metadata is inconsistent")]
pub struct InvalidBibliographyImportSource;

impl BibliographyImportSource {
    pub fn create(
        source_id: BibliographyImportSourceId,
        owner: &Identity,
        display_name: String,
        imported_at: UnixMillis,
    ) -> Result<Self, InvalidBibliographyImportSource> {
        let source = Self {
            source_id,
            owner: owner.clone(),
            method: BibliographyImportMethod::CslJsonFile,
            display_name,
            revision: Revision::INITIAL,
            created_at: imported_at,
            last_imported_at: imported_at,
        };
        source.validate()?;
        Ok(source)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        source_id: BibliographyImportSourceId,
        owner: Identity,
        method: BibliographyImportMethod,
        display_name: String,
        revision: Revision,
        created_at: UnixMillis,
        last_imported_at: UnixMillis,
    ) -> Result<Self, InvalidBibliographyImportSource> {
        let source = Self {
            source_id,
            owner,
            method,
            display_name,
            revision,
            created_at,
            last_imported_at,
        };
        source.validate()?;
        Ok(source)
    }

    pub const fn source_id(&self) -> BibliographyImportSourceId {
        self.source_id
    }

    pub const fn owner(&self) -> &Identity {
        &self.owner
    }

    pub const fn method(&self) -> BibliographyImportMethod {
        self.method
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub const fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    pub const fn last_imported_at(&self) -> UnixMillis {
        self.last_imported_at
    }

    fn validate(&self) -> Result<(), InvalidBibliographyImportSource> {
        if self.display_name.is_empty()
            || self.display_name.trim() != self.display_name
            || self.display_name.chars().count() > MAX_BIBLIOGRAPHY_IMPORT_SOURCE_NAME_CHARACTERS
            || self.display_name.chars().any(char::is_control)
            || self.last_imported_at < self.created_at
        {
            return Err(InvalidBibliographyImportSource);
        }
        Ok(())
    }
}

/// 正規化したCSL-JSONから求めたSHA-256。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BibliographyContentDigest([u8; 32]);

impl BibliographyContentDigest {
    pub const fn new(value: [u8; 32]) -> Self {
        Self(value)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// 取込元の一項目とMarginalis内の書誌項目の対応。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BibliographyImportLink {
    source_id: BibliographyImportSourceId,
    external_item_id: String,
    item_id: BibliographyItemId,
    imported_digest: BibliographyContentDigest,
    imported_item_revision: Revision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("bibliography import link metadata is inconsistent")]
pub struct InvalidBibliographyImportLink;

impl BibliographyImportLink {
    pub fn new(
        source_id: BibliographyImportSourceId,
        external_item_id: String,
        item_id: BibliographyItemId,
        imported_digest: BibliographyContentDigest,
        imported_item_revision: Revision,
    ) -> Result<Self, InvalidBibliographyImportLink> {
        if external_item_id.is_empty()
            || external_item_id.len() > MAX_BIBLIOGRAPHY_EXTERNAL_ITEM_ID_BYTES
            || !external_item_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
        {
            return Err(InvalidBibliographyImportLink);
        }
        Ok(Self {
            source_id,
            external_item_id,
            item_id,
            imported_digest,
            imported_item_revision,
        })
    }

    pub const fn source_id(&self) -> BibliographyImportSourceId {
        self.source_id
    }

    pub fn external_item_id(&self) -> &str {
        &self.external_item_id
    }

    pub const fn item_id(&self) -> BibliographyItemId {
        self.item_id
    }

    pub const fn imported_digest(&self) -> BibliographyContentDigest {
        self.imported_digest
    }

    pub const fn imported_item_revision(&self) -> Revision {
        self.imported_item_revision
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn source_id() -> BibliographyImportSourceId {
        BibliographyImportSourceId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-0000000000a1").expect("UUIDv7"),
        )
    }

    #[test]
    fn source_rejects_empty_name_and_time_before_creation() {
        let owner =
            Identity::new("https://id.example.test".into(), "alice".into()).expect("identity");
        assert!(
            BibliographyImportSource::restore(
                source_id(),
                owner.clone(),
                BibliographyImportMethod::CslJsonFile,
                String::new(),
                Revision::INITIAL,
                UnixMillis::new(10),
                UnixMillis::new(10),
            )
            .is_err()
        );
        assert!(
            BibliographyImportSource::create(
                source_id(),
                &Identity::new("https://id.example.test".into(), "alice".into()).expect("identity"),
                " Zo\ttero ".into(),
                UnixMillis::new(10),
            )
            .is_err()
        );
        assert!(
            BibliographyImportSource::restore(
                source_id(),
                owner,
                BibliographyImportMethod::CslJsonFile,
                "Zotero".into(),
                Revision::INITIAL,
                UnixMillis::new(10),
                UnixMillis::new(9),
            )
            .is_err()
        );
    }

    #[test]
    fn link_requires_an_external_item_id() {
        let item_id = BibliographyItemId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-0000000000a2").expect("UUIDv7"),
        );
        assert_eq!(
            BibliographyImportLink::new(
                source_id(),
                String::new(),
                item_id,
                BibliographyContentDigest::new([0; 32]),
                Revision::INITIAL,
            ),
            Err(InvalidBibliographyImportLink)
        );
        assert_eq!(
            BibliographyImportLink::new(
                source_id(),
                "invalid external id".into(),
                item_id,
                BibliographyContentDigest::new([0; 32]),
                Revision::INITIAL,
            ),
            Err(InvalidBibliographyImportLink)
        );
    }
}
