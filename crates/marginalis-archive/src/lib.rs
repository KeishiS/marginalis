//! ノート、ACL、書誌情報を可搬な形へ書き出す出力形式。
//!
//! 復元へ使う保存形式（[`Archive`]）と、他の道具で読むための出力（[`documents`]）を持ちます。
//! 形式そのものの定義（版、移行できる旧契約）はこのcrateが持ちます。一方、ノート本文を現行規則で
//! 再検証する処理は具体的な解析器に依存するため、[`NoteContent`] portとして受け取ります。
//! どの解析器を使うかはcomposition rootが決めます。

pub mod documents;

use marginalis_application::{
    InvalidSnapshot, LogicalSnapshot, MathMacro, MathMacroSettings, MathMacroSettingsSnapshot,
    NoteAclSnapshotEntry, NoteContent,
};
use marginalis_domain::{
    BibliographyContentDigest, BibliographyImportLink, BibliographyImportMethod,
    BibliographyImportSource, BibliographyImportSourceId, BibliographyItem, BibliographyItemId,
    EntityId, Identity, Note, NoteCreationSource, NoteDraft, NoteId, NotePermission, NoteRestore,
    NoteReviewRecord, NoteReviewTracking, Revision, UnixMillis,
};
use serde::{Deserialize, Serialize};

/// archiveの構造を表す形式名。
///
/// 項目の追加、削除または意味の変更で上げます。AdocWeave package版とnote profile版は
/// manifestへ別の項目として記録するため、解析器だけが変わった場合は形式名を変えません。
pub const ARCHIVE_FORMAT: &str = "marginalis-archive-17";
/// archive内のノートを受理できる入力規則の版。
///
/// 受理する本文が変わったときに上げます。版4までのノートはタグを`:tags:`で並べていました。
/// 版5では独自属性へ接頭辞を付け、`:marginalis-tags:`へ変わっています。
pub const ARCHIVE_NOTE_PROFILE_VERSION: u32 = 5;
/// 独自の文書属性へ接頭辞を付ける前の入力規則の版。
///
/// これ以下の版で書き出したarchiveは、本文の属性名を現行の名前へ直してから再検証します。
const UNPREFIXED_ATTRIBUTE_PROFILE_VERSION: u32 = 4;
#[derive(Clone, Copy, Debug)]
struct MigrationContract {
    format: &'static str,
    adocweave_package_version: &'static str,
    note_profile_version: u32,
    has_provenance: bool,
    has_bibliography_import_baseline: bool,
}

const fn migration_contract(
    format: &'static str,
    adocweave_package_version: &'static str,
    note_profile_version: u32,
    has_provenance: bool,
    has_bibliography_import_baseline: bool,
) -> MigrationContract {
    MigrationContract {
        format,
        adocweave_package_version,
        note_profile_version,
        has_provenance,
        has_bibliography_import_baseline,
    }
}

/// 移行元として受理する旧archive契約。形式、AdocWeave package版、note profile版の組。
const SUPPORTED_MIGRATION_CONTRACTS: &[MigrationContract] = &[
    migration_contract("marginalis-archive-17", "0.33.0", 5, true, true),
    migration_contract("marginalis-archive-16", "0.27.0", 5, true, true),
    migration_contract("marginalis-archive-15", "0.27.0", 5, true, false),
    migration_contract("marginalis-archive-14", "0.27.0", 5, false, false),
    migration_contract("marginalis-archive-13", "0.27.0", 5, false, false),
    migration_contract("marginalis-archive-13", "0.23.0", 5, false, false),
    migration_contract("marginalis-archive-13", "0.23.0", 4, false, false),
    migration_contract("marginalis-archive-12", "0.22.0", 4, false, false),
    migration_contract("marginalis-archive-11", "0.20.0", 4, false, false),
    migration_contract("marginalis-archive-10", "0.20.0", 4, false, false),
    migration_contract("marginalis-archive-9", "0.19.0", 4, false, false),
    migration_contract("marginalis-archive-8", "0.17.0", 4, false, false),
    migration_contract("marginalis-archive-7", "0.11.0", 3, false, false),
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
    #[serde(default)]
    pub bibliography_import_sources: Vec<ArchiveBibliographyImportSource>,
    #[serde(default)]
    pub bibliography_import_links: Vec<ArchiveBibliographyImportLink>,
    #[serde(default)]
    pub math_macro_settings: Vec<ArchiveMathMacroSettings>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ArchiveNoteProvenance>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveNoteProvenance {
    pub created_via: NoteCreationSource,
    pub review_tracking_known: bool,
    pub reviewed_revision: Option<i64>,
    pub reviewed_at_ms: Option<i64>,
    pub reviewer_issuer: Option<String>,
    pub reviewer_subject: Option<String>,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveBibliographyImportSource {
    pub source_id: String,
    pub owner_issuer: String,
    pub owner_subject: String,
    pub method: BibliographyImportMethod,
    pub display_name: String,
    pub revision: i64,
    pub created_at_ms: i64,
    pub last_imported_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveBibliographyImportLink {
    pub source_id: String,
    pub external_item_id: String,
    pub item_id: String,
    pub imported_digest_sha256: String,
    pub imported_item_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveMathMacroSettings {
    pub owner_issuer: String,
    pub owner_subject: String,
    pub macros: Vec<ArchiveMathMacro>,
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveMathMacro {
    pub name: String,
    pub replacement: String,
    pub argument_count: u8,
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
        self.bibliography_import_sources
            .sort_by(|left, right| left.source_id.cmp(&right.source_id));
        self.bibliography_import_links.sort_by(|left, right| {
            (&left.source_id, &left.external_item_id)
                .cmp(&(&right.source_id, &right.external_item_id))
        });
        self.math_macro_settings.sort_by(|left, right| {
            (&left.owner_issuer, &left.owner_subject)
                .cmp(&(&right.owner_issuer, &right.owner_subject))
        });
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
                provenance: Some(ArchiveNoteProvenance {
                    created_via: note.created_via(),
                    review_tracking_known: note.review_tracking_known(),
                    reviewed_revision: note.last_review().map(|review| review.revision().get()),
                    reviewed_at_ms: note.last_review().map(|review| review.reviewed_at().get()),
                    reviewer_issuer: note
                        .last_review()
                        .map(|review| review.reviewer().issuer().to_owned()),
                    reviewer_subject: note
                        .last_review()
                        .map(|review| review.reviewer().subject().to_owned()),
                }),
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
        bibliography_import_sources: snapshot
            .bibliography_import_sources()
            .iter()
            .map(|source| ArchiveBibliographyImportSource {
                source_id: source.source_id().to_string(),
                owner_issuer: source.owner().issuer().to_owned(),
                owner_subject: source.owner().subject().to_owned(),
                method: source.method(),
                display_name: source.display_name().to_owned(),
                revision: source.revision().get(),
                created_at_ms: source.created_at().get(),
                last_imported_at_ms: source.last_imported_at().get(),
            })
            .collect(),
        bibliography_import_links: snapshot
            .bibliography_import_links()
            .iter()
            .map(|link| ArchiveBibliographyImportLink {
                source_id: link.source_id().to_string(),
                external_item_id: link.external_item_id().to_owned(),
                item_id: link.item_id().to_string(),
                imported_digest_sha256: encode_digest(link.imported_digest()),
                imported_item_revision: link.imported_item_revision().get(),
            })
            .collect(),
        math_macro_settings: snapshot
            .math_macro_settings()
            .iter()
            .map(|entry| ArchiveMathMacroSettings {
                owner_issuer: entry.owner().issuer().to_owned(),
                owner_subject: entry.owner().subject().to_owned(),
                macros: entry
                    .settings()
                    .macros
                    .iter()
                    .map(|item| ArchiveMathMacro {
                        name: item.name.clone(),
                        replacement: item.replacement.clone(),
                        argument_count: item.argument_count,
                    })
                    .collect(),
                revision: entry.settings().revision,
            })
            .collect(),
    }
}

fn encode_digest(digest: BibliographyContentDigest) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in digest.as_bytes() {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn decode_digest(encoded: &str) -> Option<BibliographyContentDigest> {
    if encoded.len() != 64 || !encoded.is_ascii() {
        return None;
    }
    let mut digest = [0_u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&encoded[index * 2..index * 2 + 2], 16).ok()?;
    }
    let digest = BibliographyContentDigest::new(digest);
    (encode_digest(digest) == encoded).then_some(digest)
}

pub fn validate_archive(
    content: &dyn NoteContent,
    archive: &Archive,
) -> Result<LogicalSnapshot, ArchiveValidationError> {
    if archive.format != ARCHIVE_FORMAT
        || archive.adocweave_package_version != content.profile().adocweave_package_version
        || archive.note_profile_version != ARCHIVE_NOTE_PROFILE_VERSION
        || archive.notes.iter().any(|note| note.provenance.is_none())
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
    let contract = SUPPORTED_MIGRATION_CONTRACTS
        .iter()
        .find(|contract| {
            archive.format == contract.format
                && archive.adocweave_package_version == contract.adocweave_package_version
                && archive.note_profile_version == contract.note_profile_version
        })
        .ok_or(ArchiveMigrationError::UnsupportedContract)?;
    if !contract.has_bibliography_import_baseline
        && (!archive.bibliography_import_sources.is_empty()
            || !archive.bibliography_import_links.is_empty())
    {
        return Err(ArchiveMigrationError::UnsupportedContract);
    }
    if let Some((position, _)) = archive
        .notes
        .iter()
        .enumerate()
        .find(|(_, note)| note.provenance.is_some() != contract.has_provenance)
    {
        // archive 15より前の契約には来歴項目がなく、archive 15以降には必ず存在する。契約を
        // 混在させた入力から、根拠のない作成経路や人手確認を引き継がない。
        return Err(ArchiveMigrationError::InvalidNote {
            position: position + 1,
        });
    }
    let archive = rename_unprefixed_attributes(archive);
    let snapshot =
        validate_archive_contents(content, &archive).map_err(ArchiveMigrationError::from)?;
    Ok(create_archive(content, &snapshot))
}

/// 接頭辞を付ける前の文書属性を、現行の名前へ直したarchiveを返す。
///
/// 現行の規則では`:tags:`が未対応の属性として拒否されるため、書き換えずに再検証すると、
/// タグを持つ既存ノートがすべて移行できません。
fn rename_unprefixed_attributes(archive: &Archive) -> Archive {
    if archive.note_profile_version > UNPREFIXED_ATTRIBUTE_PROFILE_VERSION {
        return archive.clone();
    }
    let mut renamed = archive.clone();
    for note in &mut renamed.notes {
        note.source = rename_tags_attribute(&note.source);
    }
    renamed
}

/// 文書headerに並ぶ`tags`属性の操作を、接頭辞付きの名前へ書き換える。
///
/// AsciiDocの文書headerは題名の行から最初の空行までで、属性の操作はその中に1行ずつ並びます。
/// 同じ並びが本文に現れても属性ではないため、headerの外は書き換えません。値の指定
/// （`:tags: 研究`）に加えて、解除の二つの記法（`:tags!:`と`:!tags:`）も対象にします。
fn rename_tags_attribute(source: &str) -> String {
    const RENAMES: [(&str, &str); 3] = [
        (":tags:", ":marginalis-tags:"),
        (":tags!:", ":marginalis-tags!:"),
        (":!tags:", ":!marginalis-tags:"),
    ];
    let mut renamed = String::with_capacity(source.len());
    let mut in_header = true;
    for line in source.split_inclusive('\n') {
        if line.trim().is_empty() {
            in_header = false;
        }
        let rename = in_header
            .then(|| {
                RENAMES
                    .iter()
                    .find_map(|(old, new)| line.strip_prefix(old).map(|rest| (*new, rest)))
            })
            .flatten();
        match rename {
            Some((new, rest)) => {
                renamed.push_str(new);
                renamed.push_str(rest);
            }
            None => renamed.push_str(line),
        }
    }
    renamed
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
            let (created_via, review) = note
                .provenance
                .as_ref()
                .map(|provenance| archive_review(provenance, &creator))
                .transpose()
                .map_err(|_| invalid_note())?
                .unwrap_or((NoteCreationSource::Unknown, NoteReviewTracking::Unknown));
            Note::restore(NoteRestore {
                note_id,
                owner: creator,
                draft: NoteDraft {
                    title: normalized.draft.title,
                    source: note.source.clone(),
                    tags: normalized.draft.tags,
                },
                created_at: UnixMillis::new(note.created_at_ms),
                updated_at: UnixMillis::new(note.updated_at_ms),
                revision,
                deleted_at: note.deleted_at_ms.map(UnixMillis::new),
                created_via,
                review,
            })
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
    let bibliography_import_sources = archive
        .bibliography_import_sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let invalid = || ArchiveContentsError::BibliographyImportSource {
                position: index + 1,
            };
            BibliographyImportSource::restore(
                source
                    .source_id
                    .parse::<EntityId>()
                    .map(BibliographyImportSourceId::new)
                    .map_err(|_| invalid())?,
                Identity::new(source.owner_issuer.clone(), source.owner_subject.clone())
                    .map_err(|_| invalid())?,
                source.method,
                source.display_name.clone(),
                Revision::new(source.revision).map_err(|_| invalid())?,
                UnixMillis::new(source.created_at_ms),
                UnixMillis::new(source.last_imported_at_ms),
            )
            .map_err(|_| invalid())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let bibliography_import_links = archive
        .bibliography_import_links
        .iter()
        .enumerate()
        .map(|(index, link)| {
            let invalid = || ArchiveContentsError::BibliographyImportLink {
                position: index + 1,
            };
            BibliographyImportLink::new(
                link.source_id
                    .parse::<EntityId>()
                    .map(BibliographyImportSourceId::new)
                    .map_err(|_| invalid())?,
                link.external_item_id.clone(),
                link.item_id
                    .parse::<EntityId>()
                    .map(BibliographyItemId::new)
                    .map_err(|_| invalid())?,
                decode_digest(&link.imported_digest_sha256).ok_or_else(invalid)?,
                Revision::new(link.imported_item_revision).map_err(|_| invalid())?,
            )
            .map_err(|_| invalid())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let math_macro_settings = archive
        .math_macro_settings
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let invalid = || ArchiveContentsError::MathMacroSettings {
                position: index + 1,
            };
            Ok(MathMacroSettingsSnapshot::new(
                Identity::new(entry.owner_issuer.clone(), entry.owner_subject.clone())
                    .map_err(|_| invalid())?,
                MathMacroSettings {
                    macros: entry
                        .macros
                        .iter()
                        .map(|item| MathMacro {
                            name: item.name.clone(),
                            replacement: item.replacement.clone(),
                            argument_count: item.argument_count,
                        })
                        .collect(),
                    revision: entry.revision,
                },
            ))
        })
        .collect::<Result<Vec<_>, ArchiveContentsError>>()?;
    LogicalSnapshot::new(notes, note_acl)
        .and_then(|snapshot| {
            snapshot.with_bibliography_data(
                bibliography_items,
                bibliography_import_sources,
                bibliography_import_links,
            )
        })
        .and_then(|snapshot| snapshot.with_math_macro_settings(math_macro_settings))
        .map_err(|error| match error {
            InvalidSnapshot::DuplicateNote { position } => ArchiveContentsError::Note { position },
            InvalidSnapshot::InvalidAclEntry { position } => {
                ArchiveContentsError::AclEntry { position }
            }
            InvalidSnapshot::InvalidReference { .. } => ArchiveContentsError::Relationships,
            InvalidSnapshot::InvalidBibliographyItem { position } => {
                ArchiveContentsError::BibliographyItem { position }
            }
            InvalidSnapshot::InvalidBibliographyImportSource { position } => {
                ArchiveContentsError::BibliographyImportSource { position }
            }
            InvalidSnapshot::InvalidBibliographyImportLink { position } => {
                ArchiveContentsError::BibliographyImportLink { position }
            }
            InvalidSnapshot::InvalidMathMacroSettings { position } => {
                ArchiveContentsError::MathMacroSettings { position }
            }
        })
}

fn archive_review(
    provenance: &ArchiveNoteProvenance,
    owner: &Identity,
) -> Result<(NoteCreationSource, NoteReviewTracking), ()> {
    let review = match (
        provenance.review_tracking_known,
        provenance.reviewed_revision,
        provenance.reviewed_at_ms,
        provenance.reviewer_issuer.as_deref(),
        provenance.reviewer_subject.as_deref(),
    ) {
        (false, None, None, None, None) => NoteReviewTracking::Unknown,
        (true, None, None, None, None) => NoteReviewTracking::pending(),
        (true, Some(revision), Some(reviewed_at), Some(issuer), Some(subject)) => {
            let reviewer = Identity::new(issuer.to_owned(), subject.to_owned()).map_err(|_| ())?;
            if &reviewer != owner {
                return Err(());
            }
            NoteReviewTracking::tracked(Some(NoteReviewRecord::new(
                Revision::new(revision).map_err(|_| ())?,
                UnixMillis::new(reviewed_at),
                reviewer,
            )))
        }
        _ => return Err(()),
    };
    Ok((provenance.created_via, review))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArchiveContentsError {
    Note { position: usize },
    AclEntry { position: usize },
    BibliographyItem { position: usize },
    BibliographyImportSource { position: usize },
    BibliographyImportLink { position: usize },
    MathMacroSettings { position: usize },
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
    #[error("archive bibliography import source at position {position} is invalid")]
    InvalidBibliographyImportSource { position: usize },
    #[error("archive bibliography import link at position {position} is invalid")]
    InvalidBibliographyImportLink { position: usize },
    #[error("archive math macro settings at position {position} are invalid")]
    InvalidMathMacroSettings { position: usize },
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
            ArchiveContentsError::BibliographyImportSource { position } => {
                Self::InvalidBibliographyImportSource { position }
            }
            ArchiveContentsError::BibliographyImportLink { position } => {
                Self::InvalidBibliographyImportLink { position }
            }
            ArchiveContentsError::MathMacroSettings { position } => {
                Self::InvalidMathMacroSettings { position }
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

    /// ノートの作成経路と人手確認記録を持たない最後の形式。
    const PRE_PROVENANCE_CONTRACT: MigrationContract =
        migration_contract("marginalis-archive-14", "0.27.0", 5, false, false);
    /// 直前の契約。AdocWeave更新時の再検証を試験する。
    ///
    /// 形式は現行と同じで、記録するAdocWeave package版だけが異なる。
    const LATEST_MIGRATION_CONTRACT: MigrationContract =
        migration_contract("marginalis-archive-17", "0.33.0", 5, true, true);

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
                    source: "= A title\n:marginalis-tags: Research\n\nsafe body".into(),
                    title: String::new(),
                    tags: Vec::new(),
                })
                .expect("draft")
                .draft,
            UnixMillis::new(0),
            NoteCreationSource::Web,
        )
    }

    #[test]
    fn archive_round_trip_preserves_notes_acl_and_math_macros() {
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
        .expect("snapshot")
        .with_math_macro_settings(vec![MathMacroSettingsSnapshot::new(
            note.owner().clone(),
            MathMacroSettings {
                macros: vec![MathMacro {
                    name: "bm".into(),
                    replacement: r"\boldsymbol{#1}".into(),
                    argument_count: 1,
                }],
                revision: 2,
            },
        )])
        .expect("math macro settings");
        let archive = create_archive(&content(), &snapshot);
        assert_eq!(archive.format, ARCHIVE_FORMAT);
        assert_eq!(archive.note_profile_version, ARCHIVE_NOTE_PROFILE_VERSION);
        assert_eq!(validate_archive(&content(), &archive), Ok(snapshot));
    }

    #[test]
    fn archive_round_trip_preserves_bibliography_import_baselines() {
        let owner = Identity::new("https://id.example.test".into(), "alice".into()).expect("owner");
        let item = BibliographyItem::create(
            BibliographyItemId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-0000000000b1").expect("UUIDv7"),
            ),
            &owner,
            "smith2026".into(),
            r#"{"id":"smith2026","title":"Example","type":"article-journal"}"#.into(),
            UnixMillis::new(10),
        );
        let source_id = BibliographyImportSourceId::new(
            EntityId::from_str("0197c9bc-0000-7000-8000-0000000000b2").expect("UUIDv7"),
        );
        let source = BibliographyImportSource::create(
            source_id,
            &owner,
            "Zotero".into(),
            UnixMillis::new(10),
        )
        .expect("source");
        let link = BibliographyImportLink::new(
            source_id,
            "external-smith".into(),
            item.item_id(),
            BibliographyContentDigest::new([0xab; 32]),
            item.revision(),
        )
        .expect("link");
        let snapshot = LogicalSnapshot::new(Vec::new(), Vec::new())
            .expect("snapshot")
            .with_bibliography_data(vec![item], vec![source], vec![link])
            .expect("bibliography import data");

        let archive = create_archive(&content(), &snapshot);
        assert_eq!(archive.bibliography_import_sources.len(), 1);
        assert_eq!(
            archive.bibliography_import_links[0].imported_digest_sha256,
            "ab".repeat(32)
        );
        assert_eq!(validate_archive(&content(), &archive), Ok(snapshot));

        let mut previous = archive.clone();
        stamp_contract(&mut previous, LATEST_MIGRATION_CONTRACT);
        assert_eq!(
            migrate_previous_archive(&content(), &previous),
            Ok(archive.clone())
        );

        let mut noncanonical_digest = archive;
        noncanonical_digest.bibliography_import_links[0].imported_digest_sha256 = "AB".repeat(32);
        assert_eq!(
            validate_archive(&content(), &noncanonical_digest),
            Err(ArchiveValidationError)
        );
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
            NoteCreationSource::Rest,
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

    #[test]
    fn archive_14_without_provenance_migrates_to_explicit_unknown_values() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, PRE_PROVENANCE_CONTRACT);

        assert_eq!(
            validate_archive(&content(), &previous),
            Err(ArchiveValidationError)
        );
        let migrated = migrate_previous_archive(&content(), &previous).expect("migration");
        let provenance = migrated.notes[0].provenance.as_ref().expect("provenance");
        assert_eq!(provenance.created_via, NoteCreationSource::Unknown);
        assert!(!provenance.review_tracking_known);
        assert_eq!(provenance.reviewed_revision, None);
    }

    /// 形式名が現行と同じでも、AdocWeave package版が違うarchiveは移行の入力として扱う。
    ///
    /// 解析器だけが変わった更新では形式名を上げないため、現行かどうかは形式名だけでは
    /// 決まらない。取り込む前に本文を現行の規則で再検証する必要がある。
    #[test]
    fn the_current_format_with_a_previous_adocweave_version_is_migrated() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let current = create_archive(&content(), &snapshot);
        let mut previous = current.clone();
        stamp_contract(&mut previous, LATEST_MIGRATION_CONTRACT);
        assert_eq!(previous.format, current.format);
        assert_ne!(
            previous.adocweave_package_version,
            current.adocweave_package_version
        );

        assert_eq!(
            validate_archive(&content(), &previous),
            Err(ArchiveValidationError)
        );
        assert_eq!(migrate_previous_archive(&content(), &previous), Ok(current));
    }

    /// 独自の文書属性へ接頭辞を付ける前の契約。
    ///
    /// 属性名の書き換えを確かめる試験は、書き換えの対象になる版を名指す必要がある。移行元の
    /// 一覧の先頭を使うと、新しい契約を加えたときに対象が変わってしまう。
    const UNPREFIXED_CONTRACT: MigrationContract = migration_contract(
        "marginalis-archive-13",
        "0.23.0",
        UNPREFIXED_ATTRIBUTE_PROFILE_VERSION,
        false,
        false,
    );

    /// archiveの契約identityを、指定した過去の組へ書き換える。
    fn stamp_contract(archive: &mut Archive, contract: MigrationContract) {
        archive.format = contract.format.into();
        archive.adocweave_package_version = contract.adocweave_package_version.into();
        archive.note_profile_version = contract.note_profile_version;
        if !contract.has_provenance {
            for note in &mut archive.notes {
                note.provenance = None;
            }
        }
    }

    #[test]
    fn every_supported_contract_is_revalidated_into_the_current_one() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let current = create_archive(&content(), &snapshot);
        let mut expected = current.clone();
        for note in &mut expected.notes {
            note.provenance = Some(ArchiveNoteProvenance {
                created_via: NoteCreationSource::Unknown,
                review_tracking_known: false,
                reviewed_revision: None,
                reviewed_at_ms: None,
                reviewer_issuer: None,
                reviewer_subject: None,
            });
        }

        for contract in SUPPORTED_MIGRATION_CONTRACTS {
            let mut historical = current.clone();
            stamp_contract(&mut historical, *contract);

            let expected = if contract.has_provenance {
                current.clone()
            } else {
                expected.clone()
            };
            assert_eq!(
                migrate_previous_archive(&content(), &historical),
                Ok(expected),
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
    fn migration_rejects_provenance_that_did_not_exist_in_the_old_contract() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut historical = create_archive(&content(), &snapshot);
        let provenance = historical.notes[0].provenance.clone();
        stamp_contract(&mut historical, PRE_PROVENANCE_CONTRACT);
        historical.notes[0].provenance = provenance;

        assert_eq!(
            migrate_previous_archive(&content(), &historical),
            Err(ArchiveMigrationError::InvalidNote { position: 1 })
        );
    }

    /// 接頭辞を付ける前の`:tags:`を書いた既存archiveが、そのまま移行できる。
    ///
    /// 書き換えずに再検証すると、タグを持つノートがすべて未対応の属性として拒否され、
    /// 既存の利用者が移行できなくなる。
    #[test]
    fn migration_renames_the_unprefixed_tags_attribute() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, UNPREFIXED_CONTRACT);
        previous.notes[0].source = "= A title\n:tags: Research\n\nsafe body".into();

        let migrated = migrate_previous_archive(&content(), &previous).expect("migrated");
        assert_eq!(
            migrated.notes[0].source,
            "= A title\n:marginalis-tags: Research\n\nsafe body"
        );
    }

    /// 書き換えるのは文書headerの属性行だけで、本文に現れた同じ並びは変えない。
    ///
    /// 本文の`:tags:`は属性の操作ではなく、ただの文字列である。書き換えると利用者が書いた
    /// 記述が変わってしまう。
    #[test]
    fn migration_leaves_the_same_text_in_the_body_untouched() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, UNPREFIXED_CONTRACT);
        previous.notes[0].source =
            "= A title\n:tags: Research\n\n以前は :tags: と書いていました。".into();

        let migrated = migrate_previous_archive(&content(), &previous).expect("migrated");
        assert_eq!(
            migrated.notes[0].source,
            "= A title\n:marginalis-tags: Research\n\n以前は :tags: と書いていました。"
        );
    }

    /// 解除の記法も書き換える。片方だけ直すと、解除が未対応の属性として拒否される。
    #[test]
    fn migration_renames_both_unset_forms_of_the_tags_attribute() {
        for (previous_source, expected) in [
            (
                "= A title\n:tags: Research\n:tags!:\n\nbody",
                "= A title\n:marginalis-tags: Research\n:marginalis-tags!:\n\nbody",
            ),
            (
                "= A title\n:tags: Research\n:!tags:\n\nbody",
                "= A title\n:marginalis-tags: Research\n:!marginalis-tags:\n\nbody",
            ),
        ] {
            let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
            let mut previous = create_archive(&content(), &snapshot);
            stamp_contract(&mut previous, UNPREFIXED_CONTRACT);
            previous.notes[0].source = previous_source.into();

            let migrated = migrate_previous_archive(&content(), &previous).expect("migrated");
            assert_eq!(migrated.notes[0].source, expected);
        }
    }

    #[test]
    fn migration_revalidates_source_under_the_current_profile() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, LATEST_MIGRATION_CONTRACT);
        previous.notes[0].source =
            "= A title\n:source-language: rust\n:marginalis-tags: {source-language}\n\nbody".into();

        let migrated = migrate_previous_archive(&content(), &previous).expect("migrated archive");
        let validated = validate_archive(&content(), &migrated).expect("current archive");
        assert_eq!(validated.notes()[0].tags(), ["rust"]);
    }

    #[test]
    fn migration_rejects_a_mixed_historical_contract_identity() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut mixed = create_archive(&content(), &snapshot);
        stamp_contract(&mut mixed, LATEST_MIGRATION_CONTRACT);
        let oldest_contract =
            SUPPORTED_MIGRATION_CONTRACTS[SUPPORTED_MIGRATION_CONTRACTS.len() - 1];
        mixed.adocweave_package_version = oldest_contract.adocweave_package_version.into();

        assert_eq!(
            migrate_previous_archive(&content(), &mixed),
            Err(ArchiveMigrationError::UnsupportedContract)
        );
    }

    #[test]
    fn migration_rejects_source_that_does_not_satisfy_the_current_profile() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, LATEST_MIGRATION_CONTRACT);
        previous.notes[0].source = concat!(
            "= A title\n:marginalis-tags: research, + \\",
            "\n  rust\n\nbody"
        )
        .into();

        assert_eq!(
            migrate_previous_archive(&content(), &previous),
            Err(ArchiveMigrationError::InvalidNote { position: 1 })
        );
    }

    #[test]
    fn migration_reports_inconsistent_note_and_acl_positions_without_identifiers() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, LATEST_MIGRATION_CONTRACT);

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
