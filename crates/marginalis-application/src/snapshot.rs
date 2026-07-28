//! 論理スナップショットと復元計画の整合性。

use std::collections::HashSet;

use marginalis_domain::{Identity, Note, NoteId, NotePermission};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteAclSnapshotEntry {
    note_id: NoteId,
    subject: String,
    permission: NotePermission,
}

impl NoteAclSnapshotEntry {
    pub fn new(note_id: NoteId, subject: String, permission: NotePermission) -> Self {
        Self {
            note_id,
            subject,
            permission,
        }
    }

    pub const fn note_id(&self) -> NoteId {
        self.note_id
    }

    pub fn subject(&self) -> &str {
        &self.subject
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSnapshot;

impl std::fmt::Display for InvalidSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("logical snapshot or restore plan is inconsistent")
    }
}

impl std::error::Error for InvalidSnapshot {}

impl LogicalSnapshot {
    pub fn new(
        notes: Vec<Note>,
        note_acl: Vec<NoteAclSnapshotEntry>,
    ) -> Result<Self, InvalidSnapshot> {
        let mut note_ids = HashSet::new();
        for note in &notes {
            if !note_ids.insert(note.note_id()) {
                return Err(InvalidSnapshot);
            }
        }

        let mut acl_keys = HashSet::new();
        for entry in &note_acl {
            let note = notes
                .iter()
                .find(|note| note.note_id() == entry.note_id)
                .ok_or(InvalidSnapshot)?;
            Identity::new(note.creator_issuer().to_owned(), entry.subject.clone())
                .map_err(|_| InvalidSnapshot)?;
            if entry.subject == note.creator_subject()
                || !acl_keys.insert((entry.note_id, entry.subject.clone()))
            {
                return Err(InvalidSnapshot);
            }
        }

        Ok(Self { notes, note_acl })
    }

    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn note_acl(&self) -> &[NoteAclSnapshotEntry] {
        &self.note_acl
    }

    pub fn into_parts(self) -> (Vec<Note>, Vec<NoteAclSnapshotEntry>) {
        (self.notes, self.note_acl)
    }
}

/// 参照索引まで検証した、一transactionで適用できる復元入力。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestorePlan {
    snapshot: LogicalSnapshot,
    references: Vec<(NoteId, NoteId)>,
}

impl RestorePlan {
    pub fn new(
        snapshot: LogicalSnapshot,
        mut references: Vec<(NoteId, NoteId)>,
    ) -> Result<Self, InvalidSnapshot> {
        let note_ids = snapshot
            .notes
            .iter()
            .map(Note::note_id)
            .collect::<HashSet<_>>();
        if references
            .iter()
            .any(|(source, _)| !note_ids.contains(source))
        {
            return Err(InvalidSnapshot);
        }
        references
            .sort_unstable_by_key(|(source, target)| (source.to_string(), target.to_string()));
        references.dedup();
        Ok(Self {
            snapshot,
            references,
        })
    }

    pub const fn snapshot(&self) -> &LogicalSnapshot {
        &self.snapshot
    }

    pub fn references(&self) -> &[(NoteId, NoteId)] {
        &self.references
    }

    pub fn into_parts(self) -> (LogicalSnapshot, Vec<(NoteId, NoteId)>) {
        (self.snapshot, self.references)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{EntityId, Identity, Revision, UnixMillis};

    use super::*;

    fn note() -> Note {
        Note::restore(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            Identity::new("https://id.example.test".into(), "alice".into()).expect("valid owner"),
            "Title".into(),
            "Body".into(),
            Vec::new(),
            UnixMillis::new(1),
            UnixMillis::new(1),
            Revision::INITIAL,
            None,
        )
        .expect("consistent note")
    }

    #[test]
    fn snapshot_rejects_dangling_duplicate_and_owner_acl_entries() {
        let note = note();
        let read = NoteAclSnapshotEntry::new(note.note_id(), "bob".into(), NotePermission::Read);
        assert!(LogicalSnapshot::new(vec![note.clone()], vec![read.clone()]).is_ok());
        assert_eq!(
            LogicalSnapshot::new(vec![note.clone(), note.clone()], Vec::new()),
            Err(InvalidSnapshot)
        );
        assert_eq!(
            LogicalSnapshot::new(vec![note.clone()], vec![read.clone(), read]),
            Err(InvalidSnapshot)
        );
        assert_eq!(
            LogicalSnapshot::new(
                vec![note.clone()],
                vec![NoteAclSnapshotEntry::new(
                    note.note_id(),
                    "alice".into(),
                    NotePermission::Edit,
                )],
            ),
            Err(InvalidSnapshot)
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
            RestorePlan::new(snapshot.clone(), vec![(missing, note.note_id())]),
            Err(InvalidSnapshot)
        );
        let plan = RestorePlan::new(
            snapshot,
            vec![(note.note_id(), missing), (note.note_id(), missing)],
        )
        .expect("restore plan");
        assert_eq!(plan.references().len(), 1);
    }
}
