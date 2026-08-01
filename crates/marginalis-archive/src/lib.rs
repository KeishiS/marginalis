//! ノート、ACL、書誌情報を可搬な形へ書き出す出力形式。
//!
//! 復元へ使う保存形式（[`Archive`]）と、他の道具で読むための出力（[`documents`]）を持ちます。
//! 形式そのものの定義（版、移行できる旧契約）はこのcrateが持ちます。一方、ノート本文を現行規則で
//! 再検証する処理は具体的な解析器に依存するため、[`NoteContent`] portとして受け取ります。
//! どの解析器を使うかはcomposition rootが決めます。

pub mod documents;

use marginalis_application::{InvalidSnapshot, LogicalSnapshot, NoteAclSnapshotEntry, NoteContent};
use marginalis_domain::{
    BibliographyItem, BibliographyItemId, EntityId, Identity, Note, NoteDraft, NoteId,
    NotePermission, Revision, UnixMillis,
};
use serde::{Deserialize, Serialize};

pub const ARCHIVE_FORMAT: &str = "marginalis-archive-13";
/// archive内のノートを受理できる入力規則の版。
pub const ARCHIVE_NOTE_PROFILE_VERSION: u32 = 4;
/// 移行元として受理する旧archive契約。形式、AdocWeave package版、note profile版の組。
const SUPPORTED_MIGRATION_CONTRACTS: &[(&str, &str, u32)] = &[
    ("marginalis-archive-12", "0.22.0", 4),
    ("marginalis-archive-11", "0.20.0", 4),
    ("marginalis-archive-10", "0.20.0", 4),
    ("marginalis-archive-9", "0.19.0", 4),
    ("marginalis-archive-8", "0.17.0", 4),
    ("marginalis-archive-7", "0.11.0", 3),
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

impl Archive {
    /// 要素を決まった順に並べ替えた同じ内容のarchiveを返す。
    ///
    /// archiveの内容はノート、ACL、書誌項目の集合であり、並びは内容の一部ではない。組み立て方に
    /// よって並びは変わるため、2つのarchiveが同じ内容かどうかを判断する前にここで揃える。
    /// 並べ替えの規則は、SQLiteから書き出すときの`ORDER BY`と同じにする。
    #[must_use]
    pub fn canonical(mut self) -> Self {
        self.notes.sort_by(|left, right| {
            // note_idは一意であるため、これだけで並びが定まる。
            left.note_id.cmp(&right.note_id)
        });
        self.note_acl.sort_by(|left, right| {
            (&left.note_id, &left.issuer, &left.subject).cmp(&(
                &right.note_id,
                &right.issuer,
                &right.subject,
            ))
        });
        self.bibliography_items
            .sort_by(|left, right| left.item_id.cmp(&right.item_id));
        self
    }
}

/// 検証済みのsnapshotを現行のarchive形式へ書き出す。
///
/// 記録するAdocWeave packageの版は、実際に検証へ使う`content`から取得する。定数を二重に
/// 持たないため、記録値と検証器が食い違わない。
pub fn create_archive(content: &dyn NoteContent, snapshot: &LogicalSnapshot) -> Archive {
    Archive {
        format: ARCHIVE_FORMAT.into(),
        adocweave_package_version: content.profile().adocweave_package_version.into(),
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

pub fn validate_archive(
    content: &dyn NoteContent,
    archive: &Archive,
) -> Result<LogicalSnapshot, ArchiveValidationError> {
    if archive.format != ARCHIVE_FORMAT
        || archive.adocweave_package_version != content.profile().adocweave_package_version
        || archive.note_profile_version != ARCHIVE_NOTE_PROFILE_VERSION
    {
        return Err(ArchiveValidationError);
    }
    validate_archive_contents(content, archive).map_err(|_| ArchiveValidationError)
}

/// 対応する旧archive契約を現行規則で全件再検証し、現行archiveへ変換する。
pub fn migrate_previous_archive(
    content: &dyn NoteContent,
    archive: &Archive,
) -> Result<Archive, ArchiveMigrationError> {
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
    let snapshot =
        validate_archive_contents(content, archive).map_err(ArchiveMigrationError::from)?;
    Ok(create_archive(content, &snapshot))
}

fn validate_archive_contents(
    content: &dyn NoteContent,
    archive: &Archive,
) -> Result<LogicalSnapshot, ArchiveContentsError> {
    let notes = archive
        .notes
        .iter()
        .enumerate()
        .map(|(index, note)| {
            let invalid_note = || ArchiveContentsError::Note {
                position: index + 1,
            };
            let normalized = content
                .validate_draft(NoteDraft {
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

    use marginalis_asciidoc::AsciiDocNoteContent;
    use marginalis_domain::{EntityId, Identity, NoteId};

    use super::*;

    /// 試験では実際の解析器を注入する。本番の依存はportだけである。
    fn content() -> AsciiDocNoteContent {
        AsciiDocNoteContent
    }

    fn note() -> Note {
        Note::create(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            &Identity::new("https://id.example.test".into(), "alice".into()).expect("owner"),
            content()
                .validate_draft(NoteDraft {
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
        let archive = create_archive(&content(), &snapshot);
        assert_eq!(archive.format, ARCHIVE_FORMAT);
        assert_eq!(archive.note_profile_version, ARCHIVE_NOTE_PROFILE_VERSION);
        assert_eq!(validate_archive(&content(), &archive), Ok(snapshot));
    }

    /// 並びだけが違うarchiveを組み立てる。内容は同じで、要素の順序だけを逆にする。
    fn reversed(mut archive: Archive) -> Archive {
        archive.notes.reverse();
        archive.note_acl.reverse();
        archive.bibliography_items.reverse();
        archive
    }

    #[test]
    fn canonical_order_makes_archives_with_the_same_content_equal() {
        let first = note();
        let second = Note::create(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000002").expect("UUIDv7"),
            ),
            &Identity::new("https://id.example.test".into(), "bob".into()).expect("owner"),
            content()
                .validate_draft(NoteDraft {
                    source: "= Another title\n\nsafe body".into(),
                    title: String::new(),
                    tags: Vec::new(),
                })
                .expect("draft")
                .draft,
            UnixMillis::new(0),
        );
        let reader = Identity::new(first.creator_issuer().into(), "reader".into()).expect("reader");
        let snapshot = LogicalSnapshot::new(
            vec![first.clone(), second.clone()],
            vec![
                NoteAclSnapshotEntry::new(first.note_id(), reader.clone(), NotePermission::Read),
                NoteAclSnapshotEntry::new(second.note_id(), reader, NotePermission::Edit),
            ],
        )
        .expect("snapshot");
        let archive = create_archive(&content(), &snapshot);

        // 並びを変えただけのarchiveは、そのままでは等しくない。
        assert_ne!(reversed(archive.clone()), archive);
        // 並びを揃えれば同じ内容だと分かる。
        assert_eq!(
            reversed(archive.clone()).canonical(),
            archive.clone().canonical()
        );
        // すでに整った並びは変わらない。
        assert_eq!(archive.clone().canonical(), archive);
    }

    #[test]
    fn canonical_order_keeps_archives_with_different_content_apart() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let archive = create_archive(&content(), &snapshot);
        let mut changed = archive.clone();
        changed.notes[0].source.push_str("\n\n追記");

        assert_ne!(changed.canonical(), archive.canonical());
    }

    #[test]
    fn archive_requires_the_exact_contract_identity() {
        let snapshot = LogicalSnapshot::new(Vec::new(), Vec::new()).expect("snapshot");
        let mut archive = create_archive(&content(), &snapshot);
        archive.note_profile_version += 1;
        assert_eq!(
            validate_archive(&content(), &archive),
            Err(ArchiveValidationError)
        );
    }

    /// archiveの契約identityを、指定した過去の組へ書き換える。
    fn stamp_contract(archive: &mut Archive, contract: (&str, &str, u32)) {
        let (format, package_version, note_profile_version) = contract;
        archive.format = format.into();
        archive.adocweave_package_version = package_version.into();
        archive.note_profile_version = note_profile_version;
    }

    #[test]
    fn every_supported_contract_is_revalidated_into_the_current_one() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let current = create_archive(&content(), &snapshot);

        for contract in SUPPORTED_MIGRATION_CONTRACTS {
            let mut historical = current.clone();
            stamp_contract(&mut historical, *contract);

            assert_eq!(
                migrate_previous_archive(&content(), &historical),
                Ok(current.clone()),
                "移行に失敗しました: {contract:?}"
            );
            assert_eq!(
                validate_archive(&content(), &historical),
                Err(ArchiveValidationError),
                "現行の契約として受理してしまいました: {contract:?}"
            );
        }
    }

    #[test]
    fn migration_revalidates_source_under_the_current_profile() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, SUPPORTED_MIGRATION_CONTRACTS[0]);
        previous.notes[0].source =
            "= A title\n:source-language: rust\n:tags: {source-language}\n\nbody".into();

        let migrated = migrate_previous_archive(&content(), &previous).expect("migrated archive");
        let validated = validate_archive(&content(), &migrated).expect("current archive");
        assert_eq!(validated.notes()[0].tags(), ["rust"]);
    }

    #[test]
    fn migration_rejects_a_mixed_historical_contract_identity() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut mixed = create_archive(&content(), &snapshot);
        stamp_contract(&mut mixed, SUPPORTED_MIGRATION_CONTRACTS[0]);
        let (_, oldest_package_version, _) =
            SUPPORTED_MIGRATION_CONTRACTS[SUPPORTED_MIGRATION_CONTRACTS.len() - 1];
        mixed.adocweave_package_version = oldest_package_version.into();

        assert_eq!(
            migrate_previous_archive(&content(), &mixed),
            Err(ArchiveMigrationError::UnsupportedContract)
        );
    }

    #[test]
    fn migration_rejects_source_that_does_not_satisfy_the_current_profile() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, SUPPORTED_MIGRATION_CONTRACTS[0]);
        previous.notes[0].source =
            concat!("= A title\n:tags: research, + \\", "\n  rust\n\nbody").into();

        assert_eq!(
            migrate_previous_archive(&content(), &previous),
            Err(ArchiveMigrationError::InvalidNote { position: 1 })
        );
    }

    #[test]
    fn migration_reports_inconsistent_note_and_acl_positions_without_identifiers() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, SUPPORTED_MIGRATION_CONTRACTS[0]);

        previous.notes.push(previous.notes[0].clone());
        assert_eq!(
            migrate_previous_archive(&content(), &previous),
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
            migrate_previous_archive(&content(), &previous),
            Err(ArchiveMigrationError::InvalidAclEntry { position: 1 })
        );
    }

    #[test]
    fn archive_rejects_invalid_authored_source() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut archive = create_archive(&content(), &snapshot);
        archive.notes[0].source = "本文だけ".into();
        assert_eq!(
            validate_archive(&content(), &archive),
            Err(ArchiveValidationError)
        );
    }
}
