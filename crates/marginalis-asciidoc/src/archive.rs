use marginalis_application::{InvalidSnapshot, LogicalSnapshot, NoteAclSnapshotEntry};
use marginalis_domain::{
    BibliographyItem, BibliographyItemId, EntityId, Identity, Note, NoteDraft, NoteId,
    NotePermission, Revision, UnixMillis,
};
use serde::{Deserialize, Serialize};

use crate::{ARCHIVE_NOTE_PROFILE_VERSION, PINNED_ADOCWEAVE_PACKAGE_VERSION, validate_note_draft};

pub const ARCHIVE_FORMAT: &str = "marginalis-archive-11";
const PREVIOUS_ARCHIVE_FORMAT: &str = "marginalis-archive-10";
const PREVIOUS_ADOCWEAVE_PACKAGE_VERSION: &str = "0.20.0";
const PREVIOUS_NOTE_PROFILE_VERSION: u32 = 4;
const INTERMEDIATE_ARCHIVE_FORMAT: &str = "marginalis-archive-9";
const INTERMEDIATE_ADOCWEAVE_PACKAGE_VERSION: &str = "0.19.0";
const INTERMEDIATE_NOTE_PROFILE_VERSION: u32 = 4;
const LEGACY_ARCHIVE_FORMAT: &str = "marginalis-archive-8";
const LEGACY_ADOCWEAVE_PACKAGE_VERSION: &str = "0.17.0";
const LEGACY_NOTE_PROFILE_VERSION: u32 = 4;
const OLDEST_ARCHIVE_FORMAT: &str = "marginalis-archive-7";
const OLDEST_ADOCWEAVE_PACKAGE_VERSION: &str = "0.11.0";
const OLDEST_NOTE_PROFILE_VERSION: u32 = 3;
const SUPPORTED_MIGRATION_CONTRACTS: &[(&str, &str, u32)] = &[
    (
        PREVIOUS_ARCHIVE_FORMAT,
        PREVIOUS_ADOCWEAVE_PACKAGE_VERSION,
        PREVIOUS_NOTE_PROFILE_VERSION,
    ),
    (
        INTERMEDIATE_ARCHIVE_FORMAT,
        INTERMEDIATE_ADOCWEAVE_PACKAGE_VERSION,
        INTERMEDIATE_NOTE_PROFILE_VERSION,
    ),
    (
        LEGACY_ARCHIVE_FORMAT,
        LEGACY_ADOCWEAVE_PACKAGE_VERSION,
        LEGACY_NOTE_PROFILE_VERSION,
    ),
    (
        OLDEST_ARCHIVE_FORMAT,
        OLDEST_ADOCWEAVE_PACKAGE_VERSION,
        OLDEST_NOTE_PROFILE_VERSION,
    ),
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub format: String,
    pub adocweave_package_version: String,
    pub note_profile_version: u32,
    pub notes: Vec<ArchiveNote>,
    pub note_acl: Vec<ArchiveAclEntry>,
    #[serde(default)]
    pub bibliography_items: Vec<ArchiveBibliographyItem>,
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
    pub permission: NotePermission,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveBibliographyItem {
    pub item_id: String,
    pub owner_issuer: String,
    pub owner_subject: String,
    pub citation_key: String,
    pub csl_json: serde_json::Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub revision: i64,
}

pub fn create_archive(snapshot: &LogicalSnapshot) -> Archive {
    Archive {
        format: ARCHIVE_FORMAT.into(),
        adocweave_package_version: PINNED_ADOCWEAVE_PACKAGE_VERSION.into(),
        note_profile_version: ARCHIVE_NOTE_PROFILE_VERSION,
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
                permission: entry.permission(),
            })
            .collect(),
        bibliography_items: snapshot
            .bibliography_items()
            .iter()
            .map(|item| ArchiveBibliographyItem {
                item_id: item.item_id().to_string(),
                owner_issuer: item.owner().issuer().to_owned(),
                owner_subject: item.owner().subject().to_owned(),
                citation_key: item.citation_key().to_owned(),
                csl_json: serde_json::from_str(item.csl_json())
                    .expect("snapshot CSL-JSON is valid"),
                created_at_ms: item.created_at().get(),
                updated_at_ms: item.updated_at().get(),
                revision: item.revision().get(),
            })
            .collect(),
    }
}

pub fn validate_archive(archive: &Archive) -> Result<LogicalSnapshot, ArchiveValidationError> {
    if archive.format != ARCHIVE_FORMAT
        || archive.adocweave_package_version != PINNED_ADOCWEAVE_PACKAGE_VERSION
        || archive.note_profile_version != ARCHIVE_NOTE_PROFILE_VERSION
    {
        return Err(ArchiveValidationError);
    }
    validate_archive_contents(archive).map_err(|_| ArchiveValidationError)
}

/// 対応する旧archive契約を現行規則で全件再検証し、現行archiveへ変換する。
pub fn migrate_previous_archive(archive: &Archive) -> Result<Archive, ArchiveMigrationError> {
    let is_supported = SUPPORTED_MIGRATION_CONTRACTS.iter().any(
        |&(format, adocweave_package_version, note_profile_version)| {
            archive.format == format
                && archive.adocweave_package_version == adocweave_package_version
                && archive.note_profile_version == note_profile_version
        },
    );
    if !is_supported {
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
                normalized.draft.title,
                note.source.clone(),
                normalized.draft.tags,
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
                entry.permission,
            ))
        })
        .collect::<Result<Vec<_>, ArchiveContentsError>>()?;
    let bibliography_items = archive
        .bibliography_items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let invalid = || ArchiveContentsError::BibliographyItem {
                position: index + 1,
            };
            let object = item.csl_json.as_object().ok_or_else(invalid)?;
            if object.get("id").and_then(serde_json::Value::as_str)
                != Some(item.citation_key.as_str())
                || object
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(str::is_empty)
            {
                return Err(invalid());
            }
            BibliographyItem::restore(
                item.item_id
                    .parse::<EntityId>()
                    .map(BibliographyItemId::new)
                    .map_err(|_| invalid())?,
                Identity::new(item.owner_issuer.clone(), item.owner_subject.clone())
                    .map_err(|_| invalid())?,
                item.citation_key.clone(),
                serde_json::to_string(&item.csl_json).map_err(|_| invalid())?,
                UnixMillis::new(item.created_at_ms),
                UnixMillis::new(item.updated_at_ms),
                Revision::new(item.revision).map_err(|_| invalid())?,
            )
            .map_err(|_| invalid())
        })
        .collect::<Result<Vec<_>, _>>()?;
    LogicalSnapshot::new(notes, note_acl)
        .and_then(|snapshot| snapshot.with_bibliography(bibliography_items))
        .map_err(|error| match error {
            InvalidSnapshot::DuplicateNote { position } => ArchiveContentsError::Note { position },
            InvalidSnapshot::InvalidAclEntry { position } => {
                ArchiveContentsError::AclEntry { position }
            }
            InvalidSnapshot::InvalidReference { .. } => ArchiveContentsError::Relationships,
            InvalidSnapshot::InvalidBibliographyItem { position } => {
                ArchiveContentsError::BibliographyItem { position }
            }
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArchiveContentsError {
    Note { position: usize },
    AclEntry { position: usize },
    BibliographyItem { position: usize },
    Relationships,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ArchiveMigrationError {
    #[error("archive is not a supported migration source")]
    UnsupportedContract,
    #[error("archive note at position {position} does not satisfy the current note profile")]
    InvalidNote { position: usize },
    #[error("archive ACL entry at position {position} is invalid")]
    InvalidAclEntry { position: usize },
    #[error("archive bibliography item at position {position} is invalid")]
    InvalidBibliographyItem { position: usize },
    #[error("archive note and ACL relationships are inconsistent")]
    InvalidRelationships,
}

impl From<ArchiveContentsError> for ArchiveMigrationError {
    fn from(error: ArchiveContentsError) -> Self {
        match error {
            ArchiveContentsError::Note { position } => Self::InvalidNote { position },
            ArchiveContentsError::AclEntry { position } => Self::InvalidAclEntry { position },
            ArchiveContentsError::BibliographyItem { position } => {
                Self::InvalidBibliographyItem { position }
            }
            ArchiveContentsError::Relationships => Self::InvalidRelationships,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("archive is inconsistent with the current archive contract")]
pub struct ArchiveValidationError;

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
            .expect("draft")
            .draft,
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
        assert_eq!(archive.note_profile_version, ARCHIVE_NOTE_PROFILE_VERSION);
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
    fn legacy_archive_is_revalidated_into_the_current_contract() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let current = create_archive(&snapshot);
        let mut legacy = current.clone();
        legacy.format = LEGACY_ARCHIVE_FORMAT.into();
        legacy.adocweave_package_version = LEGACY_ADOCWEAVE_PACKAGE_VERSION.into();
        legacy.note_profile_version = LEGACY_NOTE_PROFILE_VERSION;

        assert_eq!(migrate_previous_archive(&legacy), Ok(current));
        assert_eq!(validate_archive(&legacy), Err(ArchiveValidationError));
    }

    #[test]
    fn oldest_archive_is_revalidated_into_the_current_contract() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let current = create_archive(&snapshot);
        let mut oldest = current.clone();
        oldest.format = OLDEST_ARCHIVE_FORMAT.into();
        oldest.adocweave_package_version = OLDEST_ADOCWEAVE_PACKAGE_VERSION.into();
        oldest.note_profile_version = OLDEST_NOTE_PROFILE_VERSION;

        assert_eq!(migrate_previous_archive(&oldest), Ok(current));
        assert_eq!(validate_archive(&oldest), Err(ArchiveValidationError));
    }

    #[test]
    fn migration_rejects_a_mixed_historical_contract_identity() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut mixed = create_archive(&snapshot);
        mixed.format = PREVIOUS_ARCHIVE_FORMAT.into();
        mixed.adocweave_package_version = LEGACY_ADOCWEAVE_PACKAGE_VERSION.into();
        mixed.note_profile_version = PREVIOUS_NOTE_PROFILE_VERSION;

        assert_eq!(
            migrate_previous_archive(&mixed),
            Err(ArchiveMigrationError::UnsupportedContract)
        );
    }

    #[test]
    fn migration_rejects_source_that_does_not_satisfy_the_current_profile() {
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
            permission: NotePermission::Edit,
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
