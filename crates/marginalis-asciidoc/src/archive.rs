use core::fmt;

use marginalis_application::{LogicalSnapshot, NoteAclSnapshotEntry};
use marginalis_domain::{
    EntityId, Identity, Note, NoteDraft, NoteId, NotePermission, Revision, UnixMillis,
};
use serde::{Deserialize, Serialize};

use crate::{NOTE_PROFILE_VERSION, PINNED_ADOCWEAVE_PACKAGE_VERSION, validate_note_draft};

pub const ARCHIVE_FORMAT: &str = "marginalis-archive-7";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub format: String,
    pub adocweave_package_version: String,
    pub note_profile_version: u32,
    pub notes: Vec<ArchiveNote>,
    pub note_acl: Vec<ArchiveAclEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveNote {
    pub note_id: String,
    pub creator_issuer: String,
    pub creator_subject: String,
    pub source: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub revision: i64,
    pub deleted_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveAclEntry {
    pub note_id: String,
    pub issuer: String,
    pub subject: String,
    pub permission: ArchivePermission,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchivePermission {
    Read,
    Edit,
}

pub fn create_archive(snapshot: &LogicalSnapshot) -> Archive {
    Archive {
        format: ARCHIVE_FORMAT.into(),
        adocweave_package_version: PINNED_ADOCWEAVE_PACKAGE_VERSION.into(),
        note_profile_version: NOTE_PROFILE_VERSION,
        notes: snapshot
            .notes()
            .iter()
            .map(|note| ArchiveNote {
                note_id: note.note_id().to_string(),
                creator_issuer: note.creator_issuer().to_owned(),
                creator_subject: note.creator_subject().to_owned(),
                source: note.source().to_owned(),
                created_at_ms: note.created_at().get(),
                updated_at_ms: note.updated_at().get(),
                revision: note.revision().get(),
                deleted_at_ms: note.deleted_at().map(UnixMillis::get),
            })
            .collect(),
        note_acl: snapshot
            .note_acl()
            .iter()
            .map(|entry| ArchiveAclEntry {
                note_id: entry.note_id().to_string(),
                issuer: entry.identity().issuer().to_owned(),
                subject: entry.identity().subject().to_owned(),
                permission: match entry.permission() {
                    NotePermission::Read => ArchivePermission::Read,
                    NotePermission::Edit => ArchivePermission::Edit,
                },
            })
            .collect(),
    }
}

pub fn validate_archive(archive: &Archive) -> Result<LogicalSnapshot, ArchiveValidationError> {
    if archive.format != ARCHIVE_FORMAT
        || archive.adocweave_package_version != PINNED_ADOCWEAVE_PACKAGE_VERSION
        || archive.note_profile_version != NOTE_PROFILE_VERSION
    {
        return Err(ArchiveValidationError);
    }
    let notes = archive
        .notes
        .iter()
        .map(|note| {
            let normalized = validate_note_draft(NoteDraft {
                source: note.source.clone(),
                title: String::new(),
                tags: Vec::new(),
            })
            .map_err(|_| ArchiveValidationError)?;
            let note_id = note
                .note_id
                .parse::<EntityId>()
                .map(NoteId::new)
                .map_err(|_| ArchiveValidationError)?;
            let creator = Identity::new(note.creator_issuer.clone(), note.creator_subject.clone())
                .map_err(|_| ArchiveValidationError)?;
            let revision = Revision::new(note.revision).map_err(|_| ArchiveValidationError)?;
            Note::restore(
                note_id,
                creator,
                normalized.title,
                note.source.clone(),
                normalized.tags,
                UnixMillis::new(note.created_at_ms),
                UnixMillis::new(note.updated_at_ms),
                revision,
                note.deleted_at_ms.map(UnixMillis::new),
            )
            .map_err(|_| ArchiveValidationError)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let note_acl = archive
        .note_acl
        .iter()
        .map(|entry| {
            let note_id = entry
                .note_id
                .parse::<EntityId>()
                .map(NoteId::new)
                .map_err(|_| ArchiveValidationError)?;
            Ok(NoteAclSnapshotEntry::new(
                note_id,
                Identity::new(entry.issuer.clone(), entry.subject.clone())
                    .map_err(|_| ArchiveValidationError)?,
                match entry.permission {
                    ArchivePermission::Read => NotePermission::Read,
                    ArchivePermission::Edit => NotePermission::Edit,
                },
            ))
        })
        .collect::<Result<Vec<_>, ArchiveValidationError>>()?;
    LogicalSnapshot::new(notes, note_acl).map_err(|_| ArchiveValidationError)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveValidationError;

impl fmt::Display for ArchiveValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("archive is inconsistent with the current archive contract")
    }
}

impl std::error::Error for ArchiveValidationError {}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{EntityId, Identity, NoteId};

    use super::*;

    fn note() -> Note {
        Note::create(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            &Identity::new("https://id.example.test".into(), "alice".into()).expect("owner"),
            validate_note_draft(NoteDraft {
                source: "= A title\n:tags: Research\n\nsafe body".into(),
                title: String::new(),
                tags: Vec::new(),
            })
            .expect("draft"),
            UnixMillis::new(0),
        )
    }

    #[test]
    fn archive_round_trip_preserves_notes_and_acl() {
        let note = note();
        let reader = Identity::new(note.creator_issuer().into(), "reader".into()).expect("reader");
        let snapshot = LogicalSnapshot::new(
            vec![note.clone()],
            vec![NoteAclSnapshotEntry::new(
                note.note_id(),
                reader,
                NotePermission::Read,
            )],
        )
        .expect("snapshot");
        let archive = create_archive(&snapshot);
        assert_eq!(archive.format, ARCHIVE_FORMAT);
        assert_eq!(validate_archive(&archive), Ok(snapshot));
    }

    #[test]
    fn archive_requires_the_exact_contract_identity() {
        let snapshot = LogicalSnapshot::new(Vec::new(), Vec::new()).expect("snapshot");
        let mut archive = create_archive(&snapshot);
        archive.note_profile_version += 1;
        assert_eq!(validate_archive(&archive), Err(ArchiveValidationError));
    }

    #[test]
    fn archive_rejects_invalid_authored_source() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut archive = create_archive(&snapshot);
        archive.notes[0].source = "本文だけ".into();
        assert_eq!(validate_archive(&archive), Err(ArchiveValidationError));
    }
}
