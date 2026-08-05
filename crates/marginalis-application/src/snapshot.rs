//! 論理スナップショットと復元計画の整合性。

use std::collections::HashSet;

use marginalis_domain::{BibliographyItem, Identity, Note, NoteId, NotePermission};

use crate::{MathMacroSettings, validate_math_macros};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MathMacroSettingsSnapshot {
    owner: Identity,
    settings: MathMacroSettings,
}

impl MathMacroSettingsSnapshot {
    pub const fn new(owner: Identity, settings: MathMacroSettings) -> Self {
        Self { owner, settings }
    }

    pub const fn owner(&self) -> &Identity {
        &self.owner
    }

    pub const fn settings(&self) -> &MathMacroSettings {
        &self.settings
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteAclSnapshotEntry {
    note_id: NoteId,
    identity: Identity,
    permission: NotePermission,
}

impl NoteAclSnapshotEntry {
    pub const fn new(note_id: NoteId, identity: Identity, permission: NotePermission) -> Self {
        Self {
            note_id,
            identity,
            permission,
        }
    }

    pub const fn note_id(&self) -> NoteId {
        self.note_id
    }

    pub const fn identity(&self) -> &Identity {
        &self.identity
    }

    pub const fn permission(&self) -> NotePermission {
        self.permission
    }
}

/// ノートとACLの相互参照を検証した、保存方式に依存しないスナップショット。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalSnapshot {
    notes: Vec<Note>,
    note_acl: Vec<NoteAclSnapshotEntry>,
    bibliography_items: Vec<BibliographyItem>,
    math_macro_settings: Vec<MathMacroSettingsSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum InvalidSnapshot {
    #[error("note at position {position} is duplicated")]
    DuplicateNote { position: usize },
    #[error("ACL entry at position {position} is inconsistent")]
    InvalidAclEntry { position: usize },
    #[error("reference at position {position} has no source note")]
    InvalidReference { position: usize },
    #[error("bibliography item at position {position} is duplicated")]
    InvalidBibliographyItem { position: usize },
    #[error("math macro settings at position {position} are invalid or duplicated")]
    InvalidMathMacroSettings { position: usize },
}

impl LogicalSnapshot {
    pub fn new(
        notes: Vec<Note>,
        note_acl: Vec<NoteAclSnapshotEntry>,
    ) -> Result<Self, InvalidSnapshot> {
        let mut note_ids = HashSet::new();
        for (index, note) in notes.iter().enumerate() {
            if !note_ids.insert(note.note_id()) {
                return Err(InvalidSnapshot::DuplicateNote {
                    position: index + 1,
                });
            }
        }

        let mut acl_keys = HashSet::new();
        for (index, entry) in note_acl.iter().enumerate() {
            let invalid_entry = InvalidSnapshot::InvalidAclEntry {
                position: index + 1,
            };
            let Some(note) = notes.iter().find(|note| note.note_id() == entry.note_id) else {
                return Err(invalid_entry);
            };
            if entry.identity.issuer() != note.creator_issuer()
                || entry.identity == *note.owner()
                || !acl_keys.insert((entry.note_id, entry.identity.clone()))
            {
                return Err(invalid_entry);
            }
        }

        Ok(Self {
            notes,
            note_acl,
            bibliography_items: Vec::new(),
            math_macro_settings: Vec::new(),
        })
    }

    pub fn with_bibliography(
        mut self,
        bibliography_items: Vec<BibliographyItem>,
    ) -> Result<Self, InvalidSnapshot> {
        let mut ids = HashSet::new();
        let mut owner_keys = HashSet::new();
        for (index, item) in bibliography_items.iter().enumerate() {
            if !ids.insert(item.item_id())
                || !owner_keys.insert((item.owner().clone(), item.citation_key().to_owned()))
            {
                return Err(InvalidSnapshot::InvalidBibliographyItem {
                    position: index + 1,
                });
            }
        }
        self.bibliography_items = bibliography_items;
        Ok(self)
    }

    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn note_acl(&self) -> &[NoteAclSnapshotEntry] {
        &self.note_acl
    }

    pub fn bibliography_items(&self) -> &[BibliographyItem] {
        &self.bibliography_items
    }

    pub fn with_math_macro_settings(
        mut self,
        settings: Vec<MathMacroSettingsSnapshot>,
    ) -> Result<Self, InvalidSnapshot> {
        let mut owners = HashSet::new();
        for (index, entry) in settings.iter().enumerate() {
            if entry.settings.revision < 1
                || !owners.insert(entry.owner.clone())
                || validate_math_macros(&entry.settings.macros).is_err()
            {
                return Err(InvalidSnapshot::InvalidMathMacroSettings {
                    position: index + 1,
                });
            }
        }
        self.math_macro_settings = settings;
        Ok(self)
    }

    pub fn math_macro_settings(&self) -> &[MathMacroSettingsSnapshot] {
        &self.math_macro_settings
    }
}

/// 参照索引まで検証した、一transactionで適用できる復元入力。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestorePlan {
    snapshot: LogicalSnapshot,
    references: Vec<(NoteId, NoteId)>,
    citations: Vec<(NoteId, String)>,
}

impl RestorePlan {
    pub fn new(
        snapshot: LogicalSnapshot,
        references: Vec<(NoteId, NoteId)>,
        citations: Vec<(NoteId, String)>,
    ) -> Result<Self, InvalidSnapshot> {
        let note_ids = snapshot
            .notes
            .iter()
            .map(Note::note_id)
            .collect::<HashSet<_>>();
        let references = sorted_links(references, &note_ids, |(source, target)| {
            (source.to_string(), target.to_string())
        })?;
        let citations = sorted_links(citations, &note_ids, |(source, key)| {
            (source.to_string(), key.clone())
        })?;
        Ok(Self {
            snapshot,
            references,
            citations,
        })
    }

    pub const fn snapshot(&self) -> &LogicalSnapshot {
        &self.snapshot
    }

    pub fn references(&self) -> &[(NoteId, NoteId)] {
        &self.references
    }

    pub fn citations(&self) -> &[(NoteId, String)] {
        &self.citations
    }
}

/// 引き元のノートが存在することを確かめ、決まった順序へ並べて重複を取り除く。
fn sorted_links<T: Clone + Eq, K: Ord>(
    mut links: Vec<(NoteId, T)>,
    note_ids: &HashSet<NoteId>,
    key: impl Fn(&(NoteId, T)) -> K,
) -> Result<Vec<(NoteId, T)>, InvalidSnapshot> {
    if let Some((index, _)) = links
        .iter()
        .enumerate()
        .find(|(_, (source, _))| !note_ids.contains(source))
    {
        return Err(InvalidSnapshot::InvalidReference {
            position: index + 1,
        });
    }
    links.sort_unstable_by_key(&key);
    links.dedup();
    Ok(links)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{
        EntityId, Identity, NoteCreationSource, NoteDraft, NoteRestore, NoteReviewTracking,
        Revision, UnixMillis,
    };

    use super::*;

    fn note() -> Note {
        Note::restore(NoteRestore {
            note_id: NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            owner: Identity::new("https://id.example.test".into(), "alice".into())
                .expect("valid owner"),
            draft: NoteDraft {
                title: "Title".into(),
                source: "Body".into(),
                tags: Vec::new(),
            },
            created_at: UnixMillis::new(1),
            updated_at: UnixMillis::new(1),
            revision: Revision::INITIAL,
            deleted_at: None,
            created_via: NoteCreationSource::Unknown,
            review: NoteReviewTracking::Unknown,
        })
        .expect("consistent note")
    }

    #[test]
    fn snapshot_rejects_dangling_duplicate_and_owner_acl_entries() {
        let note = note();
        let bob = Identity::new("https://id.example.test".into(), "bob".into()).expect("identity");
        let read = NoteAclSnapshotEntry::new(note.note_id(), bob, NotePermission::Read);
        assert!(LogicalSnapshot::new(vec![note.clone()], vec![read.clone()]).is_ok());
        assert_eq!(
            LogicalSnapshot::new(vec![note.clone(), note.clone()], Vec::new()),
            Err(InvalidSnapshot::DuplicateNote { position: 2 })
        );
        assert_eq!(
            LogicalSnapshot::new(vec![note.clone()], vec![read.clone(), read]),
            Err(InvalidSnapshot::InvalidAclEntry { position: 2 })
        );
        assert_eq!(
            LogicalSnapshot::new(
                vec![note.clone()],
                vec![NoteAclSnapshotEntry::new(
                    note.note_id(),
                    note.owner().clone(),
                    NotePermission::Edit,
                )],
            ),
            Err(InvalidSnapshot::InvalidAclEntry { position: 1 })
        );
    }

    #[test]
    fn restore_plan_rejects_a_missing_source_and_normalizes_duplicates() {
        let note = note();
        let snapshot = LogicalSnapshot::new(vec![note.clone()], Vec::new()).expect("snapshot");
        let missing = NoteId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-000000000002").expect("UUIDv7"),
        );
        assert_eq!(
            RestorePlan::new(
                snapshot.clone(),
                vec![(missing, note.note_id())],
                Vec::new()
            ),
            Err(InvalidSnapshot::InvalidReference { position: 1 })
        );
        let plan = RestorePlan::new(
            snapshot,
            vec![(note.note_id(), missing), (note.note_id(), missing)],
            vec![
                (note.note_id(), "smith2024".to_owned()),
                (note.note_id(), "smith2024".to_owned()),
            ],
        )
        .expect("restore plan");
        assert_eq!(plan.references().len(), 1);
        // 引用も同じ規則で重複を取り除く。
        assert_eq!(plan.citations().len(), 1);
    }
}
