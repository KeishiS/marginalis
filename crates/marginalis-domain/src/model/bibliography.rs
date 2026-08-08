//! 書誌情報の識別子と正本。

use core::fmt;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("bibliography item metadata is inconsistent")]
pub struct InvalidBibliographyItem;

impl BibliographyItem {
    pub fn create(
        item_id: BibliographyItemId,
        owner: &Identity,
        citation_key: String,
        csl_json: String,
        created_at: UnixMillis,
    ) -> Self {
        Self {
            item_id,
            owner: owner.clone(),
            citation_key,
            csl_json,
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
        if created_at > updated_at || citation_key.is_empty() || csl_json.is_empty() {
            return Err(InvalidBibliographyItem);
        }
        Ok(Self {
            item_id,
            owner,
            citation_key,
            csl_json,
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
