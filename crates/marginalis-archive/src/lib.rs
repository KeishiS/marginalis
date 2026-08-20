//! ノート、ACL、文献情報を可搬な形へ書き出す出力形式。
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
    NoteReviewRecord, NoteReviewTracking, Principal, PrincipalId, PrincipalRef, Revision,
    UnixMillis,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// archiveの構造を表す形式名。
///
/// 項目の追加、削除または意味の変更で上げます。AdocWeave package版とnote profile版は
/// manifestへ別の項目として記録するため、解析器だけが変わった場合は形式名を変えません。
pub const ARCHIVE_FORMAT: &str = "marginalis-archive-18";
/// archive内のノートを受理できる入力規則の版。
///
/// 受理する本文が変わったときに上げます。版4までのノートはタグを`:tags:`で並べていました。
/// 版5では独自属性へ接頭辞を付け、`:marginalis-tags:`へ変わっています。
pub const ARCHIVE_NOTE_PROFILE_VERSION: u32 = 5;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MigrationContract {
    format: &'static str,
    adocweave_package_version: &'static str,
    note_profile_version: u32,
}

const fn migration_contract(
    format: &'static str,
    adocweave_package_version: &'static str,
    note_profile_version: u32,
) -> MigrationContract {
    MigrationContract {
        format,
        adocweave_package_version,
        note_profile_version,
    }
}

impl MigrationContract {
    fn matches(self, archive: &Archive) -> bool {
        archive.format == self.format
            && archive.adocweave_package_version == self.adocweave_package_version
            && archive.note_profile_version == self.note_profile_version
    }
}

/// 移行元として受理する、現行契約の直前に公開されたarchive契約。
///
/// サポート方針(ADR 0018): 現行バイナリが変換する旧契約は、この1件だけとする。それより古い
/// archiveは、対応していた公開済みリリースを使って隣接する契約間を順番に変換する。
/// v0.46.0からv0.47.0が書き出した契約。
const PREVIOUS_MIGRATION_CONTRACT: MigrationContract =
    migration_contract("marginalis-archive-17", "0.41.0", 5);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub format: String,
    pub adocweave_package_version: String,
    pub note_profile_version: u32,
    /// 内部IDを含まない、代表identityとalias群の対応。
    ///
    /// 旧契約では項目自体がなく、現行契約では業務データが空でも空配列を必須とする。
    /// `Option`はこの契約差を読み分けるためだけに使い、現行archiveは常に`Some`で書き出す。
    #[serde(
        default,
        deserialize_with = "deserialize_optional_principals",
        skip_serializing_if = "Option::is_none"
    )]
    pub principals: Option<Vec<ArchivePrincipal>>,
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
pub struct ArchivePrincipal {
    pub primary_issuer: String,
    pub primary_subject: String,
    pub aliases: Vec<ArchiveIdentity>,
}

fn deserialize_optional_principals<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<ArchivePrincipal>>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<ArchivePrincipal>::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveIdentity {
    pub issuer: String,
    pub subject: String,
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
    /// archiveの内容はノート、ACL、文献項目の集合であり、並びは内容の一部ではない。組み立て方に
    /// よって並びは変わるため、2つのarchiveが同じ内容かどうかを判断する前にここで揃える。
    /// 並べ替えの規則は、SQLiteから書き出すときの`ORDER BY`と同じにする。
    #[must_use]
    pub fn canonical(mut self) -> Self {
        if let Some(principals) = &mut self.principals {
            for principal in &mut *principals {
                principal.aliases.sort_by(|left, right| {
                    (&left.issuer, &left.subject).cmp(&(&right.issuer, &right.subject))
                });
            }
            principals.sort_by(|left, right| {
                (&left.primary_issuer, &left.primary_subject)
                    .cmp(&(&right.primary_issuer, &right.primary_subject))
            });
        }
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
        principals: Some(
            snapshot
                .principals()
                .iter()
                .map(|principal| ArchivePrincipal {
                    primary_issuer: principal.primary_identity().issuer().to_owned(),
                    primary_subject: principal.primary_identity().subject().to_owned(),
                    aliases: principal
                        .identities()
                        .iter()
                        .filter(|identity| *identity != principal.primary_identity())
                        .map(|identity| ArchiveIdentity {
                            issuer: identity.issuer().to_owned(),
                            subject: identity.subject().to_owned(),
                        })
                        .collect(),
                })
                .collect(),
        ),
        notes: snapshot
            .notes()
            .iter()
            .map(|note| ArchiveNote {
                note_id: note.note_id().to_string(),
                creator_issuer: note.owner().primary_identity().issuer().to_owned(),
                creator_subject: note.owner().primary_identity().subject().to_owned(),
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
                        .map(|review| review.reviewer().primary_identity().issuer().to_owned()),
                    reviewer_subject: note
                        .last_review()
                        .map(|review| review.reviewer().primary_identity().subject().to_owned()),
                }),
            })
            .collect(),
        note_acl: snapshot
            .note_acl()
            .iter()
            .map(|entry| ArchiveAclEntry {
                note_id: entry.note_id().to_string(),
                issuer: entry.principal().primary_identity().issuer().to_owned(),
                subject: entry.principal().primary_identity().subject().to_owned(),
                permission: entry.permission(),
            })
            .collect(),
        bibliography_items: snapshot
            .bibliography_items()
            .iter()
            .map(|item| ArchiveBibliographyItem {
                item_id: item.item_id().to_string(),
                owner_issuer: item.owner().primary_identity().issuer().to_owned(),
                owner_subject: item.owner().primary_identity().subject().to_owned(),
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
                owner_issuer: source.owner().primary_identity().issuer().to_owned(),
                owner_subject: source.owner().primary_identity().subject().to_owned(),
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
                owner_issuer: entry.owner().primary_identity().issuer().to_owned(),
                owner_subject: entry.owner().primary_identity().subject().to_owned(),
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
    .canonical()
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
    validate_archive_contents(content, archive, PrincipalEncoding::Declared)
        .map_err(|_| ArchiveValidationError)
}

/// 対応する旧archive契約を現行規則で全件再検証し、現行archiveへ変換する。
pub fn migrate_previous_archive(
    content: &dyn NoteContent,
    archive: &Archive,
) -> Result<Archive, ArchiveMigrationError> {
    if !PREVIOUS_MIGRATION_CONTRACT.matches(archive) {
        return Err(ArchiveMigrationError::UnsupportedContract);
    }
    if archive.principals.is_some() {
        // 直前契約にはprincipal群が存在しない。形式名だけを旧契約へ書き換えた入力や、
        // 新旧の項目を混ぜた入力を受理しない。
        return Err(ArchiveMigrationError::InvalidPrincipal { position: 1 });
    }
    if let Some((position, _)) = archive
        .notes
        .iter()
        .enumerate()
        .find(|(_, note)| note.provenance.is_none())
    {
        // 対応契約には来歴項目が必ず存在する。欠落した入力から、根拠のない作成経路や
        // 人手確認を引き継がない。
        return Err(ArchiveMigrationError::InvalidNote {
            position: position + 1,
        });
    }
    let snapshot = validate_archive_contents(content, archive, PrincipalEncoding::Legacy)
        .map_err(ArchiveMigrationError::from)?;
    Ok(create_archive(content, &snapshot))
}

#[derive(Clone, Copy)]
enum PrincipalEncoding {
    Declared,
    Legacy,
}

fn validate_archive_contents(
    content: &dyn NoteContent,
    archive: &Archive,
    principal_encoding: PrincipalEncoding,
) -> Result<LogicalSnapshot, ArchiveContentsError> {
    let archive_principals = match principal_encoding {
        PrincipalEncoding::Declared => declared_archive_principals(archive)?,
        PrincipalEncoding::Legacy => legacy_archive_principals(archive)?,
    };
    let principal_refs = &archive_principals.references;
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
            let creator =
                principal_ref(principal_refs, &note.creator_issuer, &note.creator_subject)
                    .map_err(|_| invalid_note())?;
            let revision = Revision::new(note.revision).map_err(|_| invalid_note())?;
            let (created_via, review) = note
                .provenance
                .as_ref()
                .map(|provenance| archive_review(provenance, &creator, principal_refs))
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
                principal_ref(principal_refs, &entry.issuer, &entry.subject)
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
            BibliographyItem::restore(
                item.item_id
                    .parse::<EntityId>()
                    .map(BibliographyItemId::new)
                    .map_err(|_| invalid())?,
                principal_ref(principal_refs, &item.owner_issuer, &item.owner_subject)
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
                principal_ref(principal_refs, &source.owner_issuer, &source.owner_subject)
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
                principal_ref(principal_refs, &entry.owner_issuer, &entry.owner_subject)
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
        .and_then(|snapshot| snapshot.with_principals(archive_principals.principals))
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
            InvalidSnapshot::InvalidPrincipal { position } => {
                ArchiveContentsError::Principal { position }
            }
            InvalidSnapshot::InvalidPrincipalReference => ArchiveContentsError::Relationships,
        })
}

fn archive_review(
    provenance: &ArchiveNoteProvenance,
    owner: &PrincipalRef,
    principal_refs: &BTreeMap<(String, String), PrincipalRef>,
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
            let reviewer = principal_ref(principal_refs, issuer, subject)?;
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

struct ArchivePrincipals {
    principals: Vec<Principal>,
    references: BTreeMap<(String, String), PrincipalRef>,
}

fn legacy_archive_principals(archive: &Archive) -> Result<ArchivePrincipals, ArchiveContentsError> {
    let identities = legacy_identity_keys(archive);
    let principals = identities
        .into_iter()
        .enumerate()
        .map(
            |(index, (issuer, subject))| -> Result<_, ArchiveContentsError> {
                let id = i64::try_from(index + 1).expect("archive identity count fits in i64");
                Ok(Principal::single(
                    PrincipalId::new(id).expect("enumerated ID is positive"),
                    Identity::new(issuer, subject)
                        .map_err(|_| ArchiveContentsError::Relationships)?,
                ))
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    let references = principals
        .iter()
        .map(|principal| {
            (
                (
                    principal.primary_identity().issuer().to_owned(),
                    principal.primary_identity().subject().to_owned(),
                ),
                principal.reference().clone(),
            )
        })
        .collect();
    Ok(ArchivePrincipals {
        principals,
        references,
    })
}

fn legacy_identity_keys(archive: &Archive) -> BTreeSet<(String, String)> {
    let mut identities = BTreeSet::new();
    for note in &archive.notes {
        identities.insert((note.creator_issuer.clone(), note.creator_subject.clone()));
        if let Some(provenance) = &note.provenance
            && let (Some(issuer), Some(subject)) = (
                provenance.reviewer_issuer.as_ref(),
                provenance.reviewer_subject.as_ref(),
            )
        {
            identities.insert((issuer.clone(), subject.clone()));
        }
    }
    identities.extend(
        archive
            .note_acl
            .iter()
            .map(|entry| (entry.issuer.clone(), entry.subject.clone())),
    );
    identities.extend(
        archive
            .bibliography_items
            .iter()
            .map(|item| (item.owner_issuer.clone(), item.owner_subject.clone())),
    );
    identities.extend(
        archive
            .bibliography_import_sources
            .iter()
            .map(|source| (source.owner_issuer.clone(), source.owner_subject.clone())),
    );
    identities.extend(
        archive
            .math_macro_settings
            .iter()
            .map(|entry| (entry.owner_issuer.clone(), entry.owner_subject.clone())),
    );
    identities
}

fn single_identity_archive_principals(archive: &Archive) -> Vec<ArchivePrincipal> {
    legacy_identity_keys(archive)
        .into_iter()
        .map(|(issuer, subject)| ArchivePrincipal {
            primary_issuer: issuer,
            primary_subject: subject,
            aliases: Vec::new(),
        })
        .collect()
}

fn principal_ref(
    principal_refs: &BTreeMap<(String, String), PrincipalRef>,
    issuer: &str,
    subject: &str,
) -> Result<PrincipalRef, ()> {
    Identity::new(issuer.to_owned(), subject.to_owned()).map_err(|_| ())?;
    principal_refs
        .get(&(issuer.to_owned(), subject.to_owned()))
        .cloned()
        .ok_or(())
}

fn declared_archive_principals(
    archive: &Archive,
) -> Result<ArchivePrincipals, ArchiveContentsError> {
    let entries = archive
        .principals
        .as_ref()
        .ok_or(ArchiveContentsError::Principal { position: 1 })?;
    let mut parsed = Vec::with_capacity(entries.len());
    let mut identities_seen = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let invalid = || ArchiveContentsError::Principal {
            position: index + 1,
        };
        let primary = Identity::new(entry.primary_issuer.clone(), entry.primary_subject.clone())
            .map_err(|_| invalid())?;
        if !identities_seen.insert((primary.issuer().to_owned(), primary.subject().to_owned())) {
            return Err(invalid());
        }
        let mut identities = vec![primary.clone()];
        for alias in &entry.aliases {
            let identity = Identity::new(alias.issuer.clone(), alias.subject.clone())
                .map_err(|_| invalid())?;
            if !identities_seen
                .insert((identity.issuer().to_owned(), identity.subject().to_owned()))
            {
                return Err(invalid());
            }
            identities.push(identity);
        }
        parsed.push((primary, identities));
    }
    parsed.sort_by(|(left, _), (right, _)| {
        (left.issuer(), left.subject()).cmp(&(right.issuer(), right.subject()))
    });

    let mut principals = Vec::with_capacity(parsed.len());
    let mut references = BTreeMap::new();
    for (index, (primary, identities)) in parsed.into_iter().enumerate() {
        let id = i64::try_from(index + 1).map_err(|_| ArchiveContentsError::Relationships)?;
        let principal = Principal::restore(
            PrincipalId::new(id).map_err(|_| ArchiveContentsError::Relationships)?,
            primary.clone(),
            identities,
        )
        .map_err(|_| ArchiveContentsError::Principal {
            position: index + 1,
        })?;
        references.insert(
            (primary.issuer().to_owned(), primary.subject().to_owned()),
            principal.reference().clone(),
        );
        principals.push(principal);
    }
    Ok(ArchivePrincipals {
        principals,
        references,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArchiveContentsError {
    Principal { position: usize },
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
    #[error("archive principal at position {position} is invalid")]
    InvalidPrincipal { position: usize },
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
            ArchiveContentsError::Principal { position } => Self::InvalidPrincipal { position },
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

    /// 試験では実際の解析器を注入する。本番の依存はportだけである。
    fn content() -> AsciiDocNoteContent {
        AsciiDocNoteContent
    }

    fn principal(subject: &str) -> PrincipalRef {
        let id = match subject {
            "alice" => 1,
            "bob" => 2,
            _ => 3,
        };
        PrincipalRef::new(
            PrincipalId::new(id).expect("ID"),
            Identity::new("https://id.example.test".into(), subject.into()).expect("identity"),
        )
    }

    fn note() -> Note {
        Note::create(
            NoteId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-000000000001").expect("UUIDv7"),
            ),
            &principal("alice"),
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
        let reader = principal("reader");
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
        let restored = validate_archive(&content(), &archive).expect("validate archive");
        assert_eq!(create_archive(&content(), &restored), archive);
    }

    #[test]
    fn archive_round_trip_preserves_primary_identity_and_aliases_without_internal_ids() {
        let old_identity = Identity::new("https://old-id.example.test".into(), "alice".into())
            .expect("old identity");
        let new_identity = Identity::new("https://new-id.example.test".into(), "alice-v2".into())
            .expect("new identity");
        let principal = Principal::restore(
            PrincipalId::new(41).expect("principal ID"),
            new_identity.clone(),
            vec![old_identity.clone(), new_identity.clone()],
        )
        .expect("principal");
        let snapshot = LogicalSnapshot::new(Vec::new(), Vec::new())
            .expect("snapshot")
            .with_principals(vec![principal])
            .expect("principal snapshot");

        let archive = create_archive(&content(), &snapshot);
        assert_eq!(
            archive.principals,
            Some(vec![ArchivePrincipal {
                primary_issuer: new_identity.issuer().into(),
                primary_subject: new_identity.subject().into(),
                aliases: vec![ArchiveIdentity {
                    issuer: old_identity.issuer().into(),
                    subject: old_identity.subject().into(),
                }],
            }])
        );
        let encoded = serde_json::to_string(&archive).expect("archive JSON");
        assert!(!encoded.contains("principal_id"));
        assert!(!encoded.contains("identity_id"));

        let mut null_principals = serde_json::to_value(&archive).expect("archive value");
        null_principals["principals"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<Archive>(null_principals).is_err());

        let mut missing_aliases = serde_json::to_value(&archive).expect("archive value");
        missing_aliases["principals"][0]
            .as_object_mut()
            .expect("principal object")
            .remove("aliases");
        assert!(serde_json::from_value::<Archive>(missing_aliases).is_err());

        let restored = validate_archive(&content(), &archive).expect("validate archive");
        assert_eq!(create_archive(&content(), &restored), archive);

        let mut duplicated = archive.clone();
        duplicated
            .principals
            .as_mut()
            .expect("current principal list")
            .push(ArchivePrincipal {
                primary_issuer: "https://third-id.example.test".into(),
                primary_subject: "other".into(),
                aliases: vec![ArchiveIdentity {
                    issuer: old_identity.issuer().into(),
                    subject: old_identity.subject().into(),
                }],
            });
        assert_eq!(
            validate_archive(&content(), &duplicated),
            Err(ArchiveValidationError)
        );
    }

    #[test]
    fn archive_round_trip_preserves_bibliography_import_baselines() {
        let owner = principal("alice");
        let item = BibliographyItem::create(
            BibliographyItemId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-0000000000b1").expect("UUIDv7"),
            ),
            &owner,
            marginalis_domain::ValidatedCslJson::new(&serde_json::json!({
                "id": "smith2026", "title": "Example", "type": "article-journal"
            }))
            .expect("valid CSL-JSON"),
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
        stamp_contract(&mut previous, PREVIOUS_MIGRATION_CONTRACT);
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
            &principal("bob"),
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
        let reader = principal("reader");
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

    /// 直前の形式は、本文規則が同じでも現行形式へ移行する。
    #[test]
    fn the_previous_format_with_the_same_note_profile_is_migrated() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let current = create_archive(&content(), &snapshot);
        let mut previous = current.clone();
        stamp_contract(&mut previous, PREVIOUS_MIGRATION_CONTRACT);
        assert_ne!(previous.format, current.format);
        assert_eq!(
            previous.adocweave_package_version,
            current.adocweave_package_version
        );

        assert_eq!(
            validate_archive(&content(), &previous),
            Err(ArchiveValidationError)
        );
        assert_eq!(migrate_previous_archive(&content(), &previous), Ok(current));
    }

    /// archiveの契約identityを、指定した過去の組へ書き換える。
    fn stamp_contract(archive: &mut Archive, contract: MigrationContract) {
        archive.format = contract.format.into();
        archive.adocweave_package_version = contract.adocweave_package_version.into();
        archive.note_profile_version = contract.note_profile_version;
        if contract == PREVIOUS_MIGRATION_CONTRACT {
            archive.principals = None;
        }
    }

    #[test]
    fn the_previous_published_contract_is_revalidated_into_the_current_one() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let current = create_archive(&content(), &snapshot);
        let mut previous = current.clone();
        stamp_contract(&mut previous, PREVIOUS_MIGRATION_CONTRACT);

        assert_eq!(migrate_previous_archive(&content(), &previous), Ok(current));
        assert_eq!(
            validate_archive(&content(), &previous),
            Err(ArchiveValidationError)
        );
    }

    /// 対応契約には来歴が必ず存在する。欠落した入力から作成経路を推測しない。
    #[test]
    fn migration_rejects_a_note_without_provenance() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut historical = create_archive(&content(), &snapshot);
        stamp_contract(&mut historical, PREVIOUS_MIGRATION_CONTRACT);
        historical.notes[0].provenance = None;

        assert_eq!(
            migrate_previous_archive(&content(), &historical),
            Err(ArchiveMigrationError::InvalidNote { position: 1 })
        );
    }

    /// サポート方針(ADR 0018)により、直前以外の契約は移行元として受理しない。
    #[test]
    fn migration_rejects_contracts_other_than_the_previous_one() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        for contract in [
            migration_contract("marginalis-archive-17", "0.40.1", 4),
            migration_contract("marginalis-archive-17", "0.40.0", 5),
            migration_contract("marginalis-archive-17", "0.36.0", 5),
            migration_contract("marginalis-archive-16", "0.27.0", 5),
            migration_contract("marginalis-archive-7", "0.11.0", 3),
        ] {
            let mut previous = create_archive(&content(), &snapshot);
            stamp_contract(&mut previous, contract);
            assert_eq!(
                migrate_previous_archive(&content(), &previous),
                Err(ArchiveMigrationError::UnsupportedContract),
                "サポート外の契約を受理しました: {contract:?}"
            );
        }
    }

    #[test]
    fn migration_revalidates_source_under_the_current_profile() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, PREVIOUS_MIGRATION_CONTRACT);
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
        stamp_contract(&mut mixed, PREVIOUS_MIGRATION_CONTRACT);
        // 形式は対応契約と同じでも、AdocWeave版がどの契約とも一致しない組は受理しない。
        mixed.adocweave_package_version = "0.39.0".into();

        assert_eq!(
            migrate_previous_archive(&content(), &mixed),
            Err(ArchiveMigrationError::UnsupportedContract)
        );
    }

    #[test]
    fn migration_rejects_source_that_does_not_satisfy_the_current_profile() {
        let snapshot = LogicalSnapshot::new(vec![note()], Vec::new()).expect("snapshot");
        let mut previous = create_archive(&content(), &snapshot);
        stamp_contract(&mut previous, PREVIOUS_MIGRATION_CONTRACT);
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
        stamp_contract(&mut previous, PREVIOUS_MIGRATION_CONTRACT);

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
