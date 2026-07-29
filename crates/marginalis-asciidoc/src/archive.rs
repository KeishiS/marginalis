use core::fmt;

use marginalis_application::{InvalidSnapshot, LogicalSnapshot, NoteAclSnapshotEntry};
use marginalis_domain::{
    EntityId, Identity, Note, NoteDraft, NoteId, NotePermission, Revision, UnixMillis,
};
use serde::{Deserialize, Serialize};

use crate::{NOTE_PROFILE_VERSION, PINNED_ADOCWEAVE_PACKAGE_VERSION, validate_note_draft};

pub const ARCHIVE_FORMAT: &str = "marginalis-archive-8";
const PREVIOUS_ARCHIVE_FORMAT: &str = "marginalis-archive-7";
const PREVIOUS_ADOCWEAVE_PACKAGE_VERSION: &str = "0.11.0";
const PREVIOUS_NOTE_PROFILE_VERSION: u32 = 3;

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
    validate_archive_contents(archive).map_err(|_| ArchiveValidationError)
}

/// 直前のarchive契約を現行規則で全件再検証し、現行archiveへ変換する。
pub fn migrate_previous_archive(archive: &Archive) -> Result<Archive, ArchiveMigrationError> {
    if archive.format != PREVIOUS_ARCHIVE_FORMAT
        || archive.adocweave_package_version != PREVIOUS_ADOCWEAVE_PACKAGE_VERSION
        || archive.note_profile_version != PREVIOUS_NOTE_PROFILE_VERSION
    {
        return Err(ArchiveMigrationError::UnsupportedContract);
    }
    let snapshot = validate_archive_contents(archive).map_err(ArchiveMigrationError::from)?;
    Ok(create_archive(&snapshot))
}

fn validate_archive_contents(archive: &Archive) -> Result<LogicalSnapshot, ArchiveContentsError> {
    let notes = archive
        .notes
        .iter()
        .enumerate()
        .map(|(index, note)| {
            let invalid_note = || ArchiveContentsError::Note {
                position: index + 1,
            };
            let normalized = validate_note_draft(NoteDraft {
                source: note.source.clone(),
                title: String::new(),
                tags: Vec::new(),
            })
            .map_err(|_| invalid_note())?;
            let note_id = note
                .note_id
                .parse::<EntityId>()
                .map(NoteId::new)
                .map_err(|_| invalid_note())?;
            let creator = Identity::new(note.creator_issuer.clone(), note.creator_subject.clone())
                .map_err(|_| invalid_note())?;
            let revision = Revision::new(note.revision).map_err(|_| invalid_note())?;
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
            .map_err(|_| invalid_note())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let note_acl = archive
        .note_acl
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let invalid_acl_entry = || ArchiveContentsError::AclEntry {
                position: index + 1,
            };
            let note_id = entry
                .note_id
                .parse::<EntityId>()
                .map(NoteId::new)
                .map_err(|_| invalid_acl_entry())?;
            Ok(NoteAclSnapshotEntry::new(
                note_id,
                Identity::new(entry.issuer.clone(), entry.subject.clone())
                    .map_err(|_| invalid_acl_entry())?,
                match entry.permission {
                    ArchivePermission::Read => NotePermission::Read,
                    ArchivePermission::Edit => NotePermission::Edit,
                },
            ))
        })
        .collect::<Result<Vec<_>, ArchiveContentsError>>()?;
    LogicalSnapshot::new(notes, note_acl).map_err(|error| match error {
        InvalidSnapshot::DuplicateNote { position } => ArchiveContentsError::Note { position },
        InvalidSnapshot::InvalidAclEntry { position } => {
            ArchiveContentsError::AclEntry { position }
        }
        InvalidSnapshot::InvalidReference { .. } => ArchiveContentsError::Relationships,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArchiveContentsError {
    Note { position: usize },
    AclEntry { position: usize },
    Relationships,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveMigrationError {
    UnsupportedContract,
    InvalidNote { position: usize },
    InvalidAclEntry { position: usize },
    InvalidRelationships,
}

impl From<ArchiveContentsError> for ArchiveMigrationError {
    fn from(error: ArchiveContentsError) -> Self {
        match error {
            ArchiveContentsError::Note { position } => Self::InvalidNote { position },
            ArchiveContentsError::AclEntry { position } => Self::InvalidAclEntry { position },
            ArchiveContentsError::Relationships => Self::InvalidRelationships,
        }
    }
}

impl fmt::Display for ArchiveMigrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedContract => {
                formatter.write_str("archive is not the immediately preceding archive contract")
            }
            Self::InvalidNote { position } => write!(
                formatter,
                "archive note at position {position} does not satisfy the current note profile"
            ),
            Self::InvalidAclEntry { position } => write!(
                formatter,
                "archive ACL entry at position {position} is invalid"
            ),
            Self::InvalidRelationships => {
                formatter.write_str("archive note and ACL relationships are inconsistent")
            }
        }
    }
}

impl std::error::Error for ArchiveMigrationError {}

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
    fn previous_archive_is_revalidated_into_the_current_contract() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let current = create_archive(&snapshot);
        let mut previous = current.clone();
        previous.format = PREVIOUS_ARCHIVE_FORMAT.into();
        previous.adocweave_package_version = PREVIOUS_ADOCWEAVE_PACKAGE_VERSION.into();
        previous.note_profile_version = PREVIOUS_NOTE_PROFILE_VERSION;

        assert_eq!(migrate_previous_archive(&previous), Ok(current));
        assert_eq!(validate_archive(&previous), Err(ArchiveValidationError));

        previous.notes[0].source =
            "= A title\n:source-language: rust\n:tags: {source-language}\n\nbody".into();
        let migrated = migrate_previous_archive(&previous).expect("migrated archive");
        let validated = validate_archive(&migrated).expect("current archive");
        assert_eq!(validated.notes()[0].tags(), ["rust"]);
    }

    #[test]
    fn migration_rejects_source_that_only_the_previous_profile_accepted() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&snapshot);
        previous.format = PREVIOUS_ARCHIVE_FORMAT.into();
        previous.adocweave_package_version = PREVIOUS_ADOCWEAVE_PACKAGE_VERSION.into();
        previous.note_profile_version = PREVIOUS_NOTE_PROFILE_VERSION;
        previous.notes[0].source =
            concat!("= A title\n:tags: research, + \\", "\n  rust\n\nbody").into();

        assert_eq!(
            migrate_previous_archive(&previous),
            Err(ArchiveMigrationError::InvalidNote { position: 1 })
        );
    }

    #[test]
    fn migration_reports_inconsistent_note_and_acl_positions_without_identifiers() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&snapshot);
        previous.format = PREVIOUS_ARCHIVE_FORMAT.into();
        previous.adocweave_package_version = PREVIOUS_ADOCWEAVE_PACKAGE_VERSION.into();
        previous.note_profile_version = PREVIOUS_NOTE_PROFILE_VERSION;

        previous.notes.push(previous.notes[0].clone());
        assert_eq!(
            migrate_previous_archive(&previous),
            Err(ArchiveMigrationError::InvalidNote { position: 2 })
        );

        previous.notes.pop();
        previous.note_acl.push(ArchiveAclEntry {
            note_id: previous.notes[0].note_id.clone(),
            issuer: previous.notes[0].creator_issuer.clone(),
            subject: previous.notes[0].creator_subject.clone(),
            permission: ArchivePermission::Edit,
        });
        assert_eq!(
            migrate_previous_archive(&previous),
            Err(ArchiveMigrationError::InvalidAclEntry { position: 1 })
        );
    }

    #[test]
    fn archive_rejects_invalid_authored_source() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut archive = create_archive(&snapshot);
        archive.notes[0].source = "本文だけ".into();
        assert_eq!(validate_archive(&archive), Err(ArchiveValidationError));
    }
}
