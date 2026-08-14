//! ノートと文献情報を、他の道具でそのまま読める形へ書き出し、編集後に取り込む形式。
//!
//! ノートは保存しているAsciiDocのまま、文献情報はCSL-JSONの配列として並べます。復元へ使う
//! [`Archive`](crate::Archive)とは目的が異なります。manifestはarchiveと同じ意味の版情報と、
//! 外部編集を検出するための状態hashを持ちます。取り込み側は、稼働しているserviceの版と比べて
//! 再検証や移行が必要かどうかを判断できます。

use std::collections::BTreeMap;

use crate::{
    Archive, ArchiveAclEntry, ArchiveBibliographyItem, ArchiveNote, ArchiveNoteProvenance,
};
use marginalis_application::LogicalSnapshot;
use marginalis_domain::{Identity, Note, NotePermission, UnixMillis};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// この出力の形式名。archiveの版とは別に管理する。
pub const DOCUMENT_EXPORT_FORMAT: &str = "marginalis-documents-2";

/// ファイル名へ残す題名の最大文字数。
///
/// 多くのファイルシステムはファイル名を255バイトまでに制限する。UTF-8では1文字が最大4バイトに
/// なるため、note IDと拡張子を加えても収まる長さへ切り詰める。
const MAX_TITLE_CHARACTERS_IN_FILE_NAME: usize = 40;

/// 書き出す内容一式。呼び出し側がこの並びどおりにファイルを作る。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentExport {
    pub manifest: DocumentManifest,
    pub files: Vec<DocumentFile>,
}

/// 出力先からの相対pathと、その内容。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentFile {
    pub path: String,
    pub contents: String,
}

/// 出力全体の版情報と、ファイルの対応。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentManifest {
    pub format: String,
    /// 書き出したMarginalisの版。
    pub marginalis_version: String,
    /// 内容を解析したAdocWeave packageの版。archiveと同じ意味を持つ。
    pub adocweave_package_version: String,
    /// ノートを受理できる入力規則の版。archiveと同じ意味を持つ。
    pub note_profile_version: u32,
    pub exported_at_ms: i64,
    /// 復元の入力がarchiveであることを、読み手へ明示する。
    pub restore_source: String,
    pub owners: Vec<DocumentOwner>,
}

/// 復元の入力がarchiveであることを示す固定文。
const RESTORE_SOURCE: &str =
    "this export is not a restore input; use export-archive and import-archive to restore";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentOwner {
    pub issuer: String,
    pub subject: String,
    /// 出力先からの相対path。issuerとsubjectから作るため、元の値とは一致しないことがある。
    pub directory: String,
    pub bibliography_file: String,
    pub notes: Vec<DocumentNote>,
    pub bibliography: Vec<DocumentBibliographyItem>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentNote {
    /// 出力先からの相対path。
    pub file: String,
    pub note_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub revision: i64,
    /// 書き出し時の本文、所有者、ACLを結び付ける変更検出用hash。署名ではない。
    pub state_sha256: String,
    pub provenance: ArchiveNoteProvenance,
    pub acl: Vec<DocumentAclEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentAclEntry {
    pub issuer: String,
    pub subject: String,
    pub permission: NotePermission,
}

/// CSL-JSONの本体はファイルへ書き出すため、manifestには所在と識別子だけを残す。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentBibliographyItem {
    pub citation_key: String,
    pub item_id: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub revision: i64,
}

/// 検証済みのsnapshotから、書き出すファイルとmanifestを組み立てる。
///
/// 削除済みのノートは出力しません。取り出した内容を別の道具で読むことが目的であり、復活を
/// 待っているノートは現在の内容ではないためです。
pub fn create_document_export(
    snapshot: &LogicalSnapshot,
    marginalis_version: &str,
    adocweave_package_version: &str,
    exported_at: UnixMillis,
) -> DocumentExport {
    let mut owners: BTreeMap<(String, String), OwnerBuilder> = BTreeMap::new();
    for note in snapshot.notes() {
        if note.deleted_at().is_some() {
            continue;
        }
        owners
            .entry(owner_key(note.owner()))
            .or_insert_with(|| OwnerBuilder::new(note.owner()))
            .notes
            .push(note);
    }
    for item in snapshot.bibliography_items() {
        owners
            .entry(owner_key(item.owner()))
            .or_insert_with(|| OwnerBuilder::new(item.owner()))
            .bibliography
            .push(item);
    }

    let mut files = Vec::new();
    let owners = owners
        .into_values()
        .map(|owner| owner.build(snapshot, &mut files))
        .collect();
    let manifest = DocumentManifest {
        format: DOCUMENT_EXPORT_FORMAT.into(),
        marginalis_version: marginalis_version.into(),
        adocweave_package_version: adocweave_package_version.into(),
        note_profile_version: crate::ARCHIVE_NOTE_PROFILE_VERSION,
        exported_at_ms: exported_at.get(),
        restore_source: RESTORE_SOURCE.into(),
        owners,
    };
    DocumentExport { manifest, files }
}

fn owner_key(identity: &Identity) -> (String, String) {
    (identity.issuer().to_owned(), identity.subject().to_owned())
}

struct OwnerBuilder<'a> {
    identity: &'a Identity,
    notes: Vec<&'a Note>,
    bibliography: Vec<&'a marginalis_domain::BibliographyItem>,
}

impl<'a> OwnerBuilder<'a> {
    fn new(identity: &'a Identity) -> Self {
        Self {
            identity,
            notes: Vec::new(),
            bibliography: Vec::new(),
        }
    }

    fn build(mut self, snapshot: &LogicalSnapshot, files: &mut Vec<DocumentFile>) -> DocumentOwner {
        let directory = owner_directory(self.identity);
        self.notes.sort_by_key(|note| note.note_id().to_string());
        self.bibliography
            .sort_by(|left, right| left.citation_key().cmp(right.citation_key()));

        let notes = self
            .notes
            .iter()
            .map(|note| {
                let file = format!("{directory}/notes/{}", note_file_name(note));
                files.push(DocumentFile {
                    path: file.clone(),
                    contents: note.source().to_owned(),
                });
                let acl = snapshot
                    .note_acl()
                    .iter()
                    .filter(|entry| entry.note_id() == note.note_id())
                    .map(|entry| DocumentAclEntry {
                        issuer: entry.identity().issuer().to_owned(),
                        subject: entry.identity().subject().to_owned(),
                        permission: entry.permission(),
                    })
                    .collect::<Vec<_>>();
                DocumentNote {
                    file,
                    note_id: note.note_id().to_string(),
                    title: note.title().to_owned(),
                    tags: note.tags().to_vec(),
                    created_at_ms: note.created_at().get(),
                    updated_at_ms: note.updated_at().get(),
                    revision: note.revision().get(),
                    state_sha256: note_state_sha256(
                        self.identity.issuer(),
                        self.identity.subject(),
                        note.source().as_bytes(),
                        &acl,
                    ),
                    provenance: ArchiveNoteProvenance {
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
                    },
                    acl,
                }
            })
            .collect();

        let bibliography_file = format!("{directory}/bibliography.json");
        files.push(DocumentFile {
            path: bibliography_file.clone(),
            contents: csl_json_array(&self.bibliography),
        });

        DocumentOwner {
            issuer: self.identity.issuer().to_owned(),
            subject: self.identity.subject().to_owned(),
            directory,
            bibliography_file,
            notes,
            bibliography: self
                .bibliography
                .iter()
                .map(|item| DocumentBibliographyItem {
                    citation_key: item.citation_key().to_owned(),
                    item_id: item.item_id().to_string(),
                    created_at_ms: item.created_at().get(),
                    updated_at_ms: item.updated_at().get(),
                    revision: item.revision().get(),
                })
                .collect(),
        }
    }
}

fn note_state_sha256(
    owner_issuer: &str,
    owner_subject: &str,
    source: &[u8],
    acl: &[DocumentAclEntry],
) -> String {
    let mut hasher = Sha256::new();
    update_hash_component(&mut hasher, owner_issuer.as_bytes());
    update_hash_component(&mut hasher, owner_subject.as_bytes());
    update_hash_component(&mut hasher, source);
    let mut entries = acl.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        (&left.issuer, &left.subject, left.permission).cmp(&(
            &right.issuer,
            &right.subject,
            right.permission,
        ))
    });
    for entry in entries {
        update_hash_component(&mut hasher, entry.issuer.as_bytes());
        update_hash_component(&mut hasher, entry.subject.as_bytes());
        hasher.update([match entry.permission {
            NotePermission::Read => 1,
            NotePermission::Edit => 2,
        }]);
    }
    format!("{:x}", hasher.finalize())
}

fn update_hash_component(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value);
}

fn is_canonical_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// 登録したCSL-JSONを、そのまま配列として並べる。
///
/// pandocやZoteroが読む形はCSL項目の配列であり、Marginalis固有の項目を足しません。内部の
/// 識別子や日時はmanifestが持ちます。
fn csl_json_array(items: &[&marginalis_domain::BibliographyItem]) -> String {
    let values = items
        .iter()
        .map(|item| {
            serde_json::from_str::<serde_json::Value>(item.csl_json())
                .expect("snapshot CSL-JSON is valid")
        })
        .collect::<Vec<_>>();
    let mut encoded =
        serde_json::to_string_pretty(&values).expect("CSL-JSON values can be encoded");
    encoded.push('\n');
    encoded
}

/// 所有者ごとのディレクトリー名。
///
/// issuerはURLであり、そのままではpathへ使えない。schemeを外し、区切りをすべて`_`へ直す。
/// 元の値はmanifestへ記録するため、ここでの変換で情報は失われない。
fn owner_directory(identity: &Identity) -> String {
    let issuer = identity
        .issuer()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    format!(
        "{}/{}",
        named_or_unnamed(issuer),
        named_or_unnamed(identity.subject())
    )
}

/// 所有者のディレクトリー名は必ず1階層になる。使える文字が残らない場合の置き換え。
fn named_or_unnamed(value: &str) -> String {
    let component = safe_path_component(value);
    if component.is_empty() {
        "unnamed".into()
    } else {
        component
    }
}

/// ノート1件のファイル名。題名とnote IDを並べる。
///
/// note IDを必ず付けるため、題名が重複しても、題名が空になっても別のファイルになる。
fn note_file_name(note: &Note) -> String {
    let title = safe_path_component(note.title());
    let title = title
        .chars()
        .take(MAX_TITLE_CHARACTERS_IN_FILE_NAME)
        .collect::<String>();
    let title = title.trim_matches(['-', '.', ' ']).to_owned();
    if title.is_empty() {
        format!("{}.adoc", note.note_id())
    } else {
        format!("{title}-{}.adoc", note.note_id())
    }
}

/// path構成要素として安全な文字列へ直す。
///
/// ディレクトリーの区切り、親ディレクトリーの指定、制御文字、ファイルシステムによって扱いが
/// 異なる記号を`-`へ置き換え、連続した`-`を1つにまとめる。使える文字が残らない場合は
/// 空文字列を返し、代わりに何を使うかは呼び出し側が決める。
fn safe_path_component(value: &str) -> String {
    let replaced = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'
                )
                || character.is_whitespace()
            {
                '-'
            } else {
                character
            }
        })
        .collect::<String>();
    let mut collapsed = String::with_capacity(replaced.len());
    for character in replaced.chars() {
        if character == '-' && collapsed.ends_with('-') {
            continue;
        }
        collapsed.push(character);
    }
    collapsed.trim_matches(['-', '.']).to_owned()
}

/// 取り込みを止める理由。
///
/// 本文や識別子を含めない。位置はmanifest内の1から始まる番号で示す。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DocumentImportError {
    #[error("document export format is not supported")]
    UnsupportedFormat,
    #[error("note at position {position} has no file in the archive")]
    MissingNoteFile { position: usize },
    #[error("bibliography file for owner at position {position} is missing")]
    MissingBibliographyFile { position: usize },
    #[error("note file at position {position} is not valid UTF-8")]
    InvalidNoteEncoding { position: usize },
    #[error("bibliography file for owner at position {position} is not valid CSL-JSON")]
    InvalidBibliography { position: usize },
    #[error("manifest entry at position {position} is inconsistent")]
    InvalidManifestEntry { position: usize },
}

/// 書き出した文書一式を、現行のarchiveへ組み立て直す。
///
/// 本文はファイル、識別子や日時はmanifestを正とする。別の道具で本文を編集してから戻せる。
/// 組み立てたarchiveは現行の契約を名乗るため、呼び出し側が現行規則で全件検証する。manifestの
/// 版が古い場合の再検証も、この検証で同時に済む。
///
/// 返すarchiveは[`Archive::canonical`]の並びにする。manifestは所有者ごとにノートをまとめるため、
/// 読んだ順のままでは`export-archive`が書き出す並びと違う。
pub fn archive_from_documents(
    manifest: &DocumentManifest,
    files: &BTreeMap<String, Vec<u8>>,
    adocweave_package_version: &str,
    imported_at: UnixMillis,
) -> Result<Archive, DocumentImportError> {
    if manifest.format != DOCUMENT_EXPORT_FORMAT {
        return Err(DocumentImportError::UnsupportedFormat);
    }
    let mut notes = Vec::new();
    let mut note_acl = Vec::new();
    let mut bibliography_items = Vec::new();
    let mut note_position = 0;

    for (owner_index, owner) in manifest.owners.iter().enumerate() {
        let owner_position = owner_index + 1;
        for note in &owner.notes {
            note_position += 1;
            let contents = files
                .get(&note.file)
                .ok_or(DocumentImportError::MissingNoteFile {
                    position: note_position,
                })?;
            let source = String::from_utf8(contents.clone()).map_err(|_| {
                DocumentImportError::InvalidNoteEncoding {
                    position: note_position,
                }
            })?;
            if !is_canonical_sha256(&note.state_sha256) {
                return Err(DocumentImportError::InvalidManifestEntry {
                    position: note_position,
                });
            }
            let state_changed = note.state_sha256
                != note_state_sha256(&owner.issuer, &owner.subject, contents, &note.acl);
            let (updated_at_ms, revision) = if state_changed {
                let revision = note.revision.checked_add(1).ok_or(
                    DocumentImportError::InvalidManifestEntry {
                        position: note_position,
                    },
                )?;
                let next_updated_at = note.updated_at_ms.checked_add(1).ok_or(
                    DocumentImportError::InvalidManifestEntry {
                        position: note_position,
                    },
                )?;
                (imported_at.get().max(next_updated_at), revision)
            } else {
                (note.updated_at_ms, note.revision)
            };
            let mut provenance = note.provenance.clone();
            if state_changed {
                provenance.review_tracking_known = true;
            }
            notes.push(ArchiveNote {
                note_id: note.note_id.clone(),
                creator_issuer: owner.issuer.clone(),
                creator_subject: owner.subject.clone(),
                source,
                created_at_ms: note.created_at_ms,
                updated_at_ms,
                revision,
                // 書き出しは削除済みノートを含まないため、取り込み後も削除済みは存在しない。
                deleted_at_ms: None,
                provenance: Some(provenance),
            });
            note_acl.extend(note.acl.iter().map(|entry| ArchiveAclEntry {
                note_id: note.note_id.clone(),
                issuer: entry.issuer.clone(),
                subject: entry.subject.clone(),
                permission: entry.permission,
            }));
        }

        let contents = files.get(&owner.bibliography_file).ok_or(
            DocumentImportError::MissingBibliographyFile {
                position: owner_position,
            },
        )?;
        let values: Vec<serde_json::Value> = serde_json::from_slice(contents).map_err(|_| {
            DocumentImportError::InvalidBibliography {
                position: owner_position,
            }
        })?;
        for item in &owner.bibliography {
            let csl_json = values
                .iter()
                .find(|value| {
                    value.get("id").and_then(serde_json::Value::as_str)
                        == Some(item.citation_key.as_str())
                })
                .ok_or(DocumentImportError::InvalidManifestEntry {
                    position: owner_position,
                })?;
            bibliography_items.push(ArchiveBibliographyItem {
                item_id: item.item_id.clone(),
                owner_issuer: owner.issuer.clone(),
                owner_subject: owner.subject.clone(),
                citation_key: item.citation_key.clone(),
                csl_json: csl_json.clone(),
                created_at_ms: item.created_at_ms,
                updated_at_ms: item.updated_at_ms,
                revision: item.revision,
            });
        }
    }

    Ok(Archive {
        format: crate::ARCHIVE_FORMAT.into(),
        adocweave_package_version: adocweave_package_version.into(),
        note_profile_version: crate::ARCHIVE_NOTE_PROFILE_VERSION,
        notes,
        note_acl,
        bibliography_items,
        // 文書書庫は復元用ではなく、外部編集した本文とCSL-JSONを読み戻す形式である。
        // 元ファイルを持たない取込元との対応と基準値は引き継がない。
        bibliography_import_sources: Vec::new(),
        bibliography_import_links: Vec::new(),
        math_macro_settings: Vec::new(),
    }
    .canonical())
}

/// manifestが記録する版が、稼働している版と違うかどうか。
///
/// 違う場合は、取り込み前に全ノートを現行規則で再検証したことを運用者へ伝える。
pub fn requires_revalidation(manifest: &DocumentManifest, adocweave_package_version: &str) -> bool {
    manifest.adocweave_package_version != adocweave_package_version
        || manifest.note_profile_version != crate::ARCHIVE_NOTE_PROFILE_VERSION
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{
        BibliographyItem, BibliographyItemId, EntityId, NoteCreationSource, NoteId, NoteRestore,
        NoteReviewRecord, NoteReviewTracking, Revision, UnixMillis,
    };

    use super::*;

    fn identity(subject: &str) -> Identity {
        Identity::new("https://id.example.test".into(), subject.into()).expect("owner")
    }

    fn note(id: &str, title: &str, owner: &str, deleted: bool) -> Note {
        Note::restore(NoteRestore {
            note_id: NoteId::new(EntityId::from_str(id).expect("UUIDv7")),
            owner: identity(owner),
            draft: marginalis_domain::NoteDraft {
                title: title.into(),
                source: format!("= {title}\n\n本文"),
                tags: vec!["研究".into()],
            },
            created_at: UnixMillis::new(1000),
            updated_at: UnixMillis::new(2000),
            revision: Revision::INITIAL,
            deleted_at: deleted.then(|| UnixMillis::new(2000)),
            created_via: NoteCreationSource::Rest,
            review: NoteReviewTracking::pending(),
        })
        .expect("note")
    }

    fn reviewed_note(id: &str, title: &str, owner: &str) -> Note {
        let owner = identity(owner);
        Note::restore(NoteRestore {
            note_id: NoteId::new(EntityId::from_str(id).expect("UUIDv7")),
            owner: owner.clone(),
            draft: marginalis_domain::NoteDraft {
                title: title.into(),
                source: format!("= {title}\n\n本文"),
                tags: vec!["研究".into()],
            },
            created_at: UnixMillis::new(1000),
            updated_at: UnixMillis::new(2000),
            revision: Revision::INITIAL,
            deleted_at: None,
            created_via: NoteCreationSource::Rest,
            review: NoteReviewTracking::tracked(Some(NoteReviewRecord::new(
                Revision::INITIAL,
                UnixMillis::new(1500),
                owner,
            ))),
        })
        .expect("reviewed note")
    }

    fn item(citation_key: &str, owner: &str) -> BibliographyItem {
        BibliographyItem::create(
            BibliographyItemId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-0000000000a1").expect("UUIDv7"),
            ),
            &identity(owner),
            marginalis_domain::ValidatedCslJson::new(
                &serde_json::json!({ "id": citation_key, "type": "book", "title": "Example" }),
            )
            .expect("valid CSL-JSON"),
            UnixMillis::new(1000),
        )
    }

    fn export(snapshot: &LogicalSnapshot) -> DocumentExport {
        create_document_export(snapshot, "0.19.0", "0.23.0", UnixMillis::new(4000))
    }

    fn identified_item(id: &str, citation_key: &str, owner: &str) -> BibliographyItem {
        BibliographyItem::create(
            BibliographyItemId::new(EntityId::from_str(id).expect("UUIDv7")),
            &identity(owner),
            marginalis_domain::ValidatedCslJson::new(
                &serde_json::json!({ "id": citation_key, "type": "book", "title": "Example" }),
            )
            .expect("valid CSL-JSON"),
            UnixMillis::new(1000),
        )
    }

    /// 書き出しをそのまま`archive_from_documents`へ渡せる形へ直す。
    fn exported_files(export: &DocumentExport) -> BTreeMap<String, Vec<u8>> {
        export
            .files
            .iter()
            .map(|file| (file.path.clone(), file.contents.clone().into_bytes()))
            .collect()
    }

    /// 所有者が2人以上でも、組み立て直したarchiveがsnapshot由来のarchiveと一致する。
    ///
    /// manifestは所有者ごとにノートをまとめるため、note IDが所有者をまたいで交互に並ぶと
    /// 読んだ順とsnapshotの順が食い違う。以前はこの違いだけで`import-documents`が中止した。
    #[test]
    fn rebuilding_matches_the_snapshot_archive_across_owners() {
        let snapshot = LogicalSnapshot::new(
            vec![
                note("0197c9bc-0000-7000-8000-000000000001", "A", "alice", false),
                note("0197c9bc-0000-7000-8000-000000000002", "B", "bob", false),
                note("0197c9bc-0000-7000-8000-000000000003", "C", "alice", false),
            ],
            Vec::new(),
        )
        .expect("snapshot")
        .with_bibliography_data(
            vec![
                // citation_keyの順とitem_idの順をわざと食い違わせる。
                identified_item(
                    "0197c9bc-0000-7000-8000-0000000000a1",
                    "tanaka2025",
                    "alice",
                ),
                identified_item("0197c9bc-0000-7000-8000-0000000000a2", "smith2024", "alice"),
            ],
            Vec::new(),
            Vec::new(),
        )
        .expect("bibliography");
        let exported = export(&snapshot);

        let rebuilt = archive_from_documents(
            &exported.manifest,
            &exported_files(&exported),
            "0.23.0",
            UnixMillis::new(5000),
        )
        .expect("rebuild");

        assert_eq!(
            rebuilt
                .notes
                .iter()
                .map(|note| note.note_id.as_str())
                .collect::<Vec<_>>(),
            [
                "0197c9bc-0000-7000-8000-000000000001",
                "0197c9bc-0000-7000-8000-000000000002",
                "0197c9bc-0000-7000-8000-000000000003",
            ]
        );
        assert_eq!(
            rebuilt
                .bibliography_items
                .iter()
                .map(|item| item.item_id.as_str())
                .collect::<Vec<_>>(),
            [
                "0197c9bc-0000-7000-8000-0000000000a1",
                "0197c9bc-0000-7000-8000-0000000000a2",
            ]
        );
    }

    #[test]
    fn notes_and_bibliography_are_grouped_by_owner() {
        let snapshot = LogicalSnapshot::new(
            vec![
                note(
                    "0197c9bc-0000-7000-8000-000000000001",
                    "先行研究の整理",
                    "alice",
                    false,
                ),
                note(
                    "0197c9bc-0000-7000-8000-000000000002",
                    "検証メモ",
                    "bob",
                    false,
                ),
            ],
            Vec::new(),
        )
        .expect("snapshot")
        .with_bibliography_data(vec![item("smith2024", "alice")], Vec::new(), Vec::new())
        .expect("snapshot with bibliography");

        let export = export(&snapshot);

        assert_eq!(export.manifest.owners.len(), 2);
        assert_eq!(export.manifest.owners[0].directory, "id.example.test/alice");
        assert_eq!(
            export.manifest.owners[0].notes[0].file,
            "id.example.test/alice/notes/先行研究の整理-0197c9bc-0000-7000-8000-000000000001.adoc"
        );
        assert_eq!(
            export.manifest.owners[1].bibliography_file,
            "id.example.test/bob/bibliography.json"
        );
        // 文献情報を持たない所有者にも空の配列を出し、ファイルの有無で意味が変わらないようにする。
        let paths = export
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"id.example.test/bob/bibliography.json"));
        assert_eq!(export.files.len(), 4);
    }

    #[test]
    fn manifest_carries_the_versions_an_import_needs() {
        let snapshot = LogicalSnapshot::new(Vec::new(), Vec::new()).expect("snapshot");

        let manifest = export(&snapshot).manifest;

        assert_eq!(manifest.format, DOCUMENT_EXPORT_FORMAT);
        assert_eq!(manifest.marginalis_version, "0.19.0");
        assert_eq!(manifest.adocweave_package_version, "0.23.0");
        assert_eq!(
            manifest.note_profile_version,
            crate::ARCHIVE_NOTE_PROFILE_VERSION
        );
        assert_eq!(manifest.exported_at_ms, 4000);
        assert!(manifest.restore_source.contains("import-archive"));
    }

    #[test]
    fn note_contents_are_the_stored_asciidoc_and_bibliography_is_a_csl_array() {
        let snapshot = LogicalSnapshot::new(
            vec![note(
                "0197c9bc-0000-7000-8000-000000000001",
                "題名",
                "alice",
                false,
            )],
            Vec::new(),
        )
        .expect("snapshot")
        .with_bibliography_data(vec![item("smith2024", "alice")], Vec::new(), Vec::new())
        .expect("snapshot with bibliography");

        let export = export(&snapshot);
        let source = export
            .files
            .iter()
            .find(|file| file.path.ends_with(".adoc"))
            .expect("note file");
        assert_eq!(source.contents, "= 題名\n\n本文");

        let bibliography = export
            .files
            .iter()
            .find(|file| file.path.ends_with("bibliography.json"))
            .expect("bibliography file");
        let values: Vec<serde_json::Value> =
            serde_json::from_str(&bibliography.contents).expect("CSL-JSON array");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0]["id"], "smith2024");
        // Marginalis固有の項目を足さない。
        assert_eq!(values[0].as_object().expect("object").len(), 3);
    }

    /// 書き出した内容を、そのまま取り込めるarchiveへ組み立て直せる。
    #[test]
    fn an_export_can_be_rebuilt_into_a_current_archive() {
        let snapshot = LogicalSnapshot::new(
            vec![note(
                "0197c9bc-0000-7000-8000-000000000001",
                "題名",
                "alice",
                false,
            )],
            Vec::new(),
        )
        .expect("snapshot")
        .with_bibliography_data(vec![item("smith2024", "alice")], Vec::new(), Vec::new())
        .expect("snapshot with bibliography");
        let export = export(&snapshot);
        let files = export
            .files
            .iter()
            .map(|file| (file.path.clone(), file.contents.as_bytes().to_vec()))
            .collect::<BTreeMap<_, _>>();

        let archive =
            archive_from_documents(&export.manifest, &files, "0.23.0", UnixMillis::new(5000))
                .expect("rebuild the archive");

        assert_eq!(archive.format, crate::ARCHIVE_FORMAT);
        assert_eq!(archive.adocweave_package_version, "0.23.0");
        assert_eq!(archive.notes.len(), 1);
        assert_eq!(archive.notes[0].source, "= 題名\n\n本文");
        assert_eq!(archive.notes[0].creator_subject, "alice");
        assert_eq!(archive.bibliography_items[0].citation_key, "smith2024");
        assert_eq!(archive.bibliography_items[0].csl_json["title"], "Example");
    }

    /// 本文を書き換えた場合はrevisionを進め、以前の人手確認を現在の版へ引き継がない。
    #[test]
    fn an_edited_note_file_replaces_the_stored_source() {
        let snapshot = LogicalSnapshot::new(
            vec![reviewed_note(
                "0197c9bc-0000-7000-8000-000000000001",
                "題名",
                "alice",
            )],
            Vec::new(),
        )
        .expect("snapshot");
        let export = export(&snapshot);
        let mut files = export
            .files
            .iter()
            .map(|file| (file.path.clone(), file.contents.as_bytes().to_vec()))
            .collect::<BTreeMap<_, _>>();
        let path = export.manifest.owners[0].notes[0].file.clone();
        files.insert(path, "= 書き換えた題名\n\n別の本文".as_bytes().to_vec());

        let archive =
            archive_from_documents(&export.manifest, &files, "0.23.0", UnixMillis::new(5000))
                .expect("rebuild the archive");

        assert_eq!(archive.notes[0].source, "= 書き換えた題名\n\n別の本文");
        assert_eq!(
            archive.notes[0].note_id,
            "0197c9bc-0000-7000-8000-000000000001"
        );
        assert_eq!(archive.notes[0].created_at_ms, 1000);
        assert_eq!(archive.notes[0].updated_at_ms, 5000);
        assert_eq!(archive.notes[0].revision, 2);
        let provenance = archive.notes[0].provenance.as_ref().expect("provenance");
        assert!(provenance.review_tracking_known);
        assert_eq!(provenance.reviewed_revision, Some(1));
    }

    /// 旧形式から移した不明状態でも、書き出し後の変更は追跡できるため確認待ちへ移す。
    #[test]
    fn editing_an_unknown_note_starts_review_tracking() {
        let snapshot = LogicalSnapshot::new(
            vec![note(
                "0197c9bc-0000-7000-8000-000000000001",
                "旧形式",
                "alice",
                false,
            )],
            Vec::new(),
        )
        .expect("snapshot");
        let export = export(&snapshot);
        let mut files = exported_files(&export);
        let path = export.manifest.owners[0].notes[0].file.clone();
        files.insert(path, "= 外部編集後\n\n本文".as_bytes().to_vec());

        let archive =
            archive_from_documents(&export.manifest, &files, "0.23.0", UnixMillis::new(5000))
                .expect("rebuild edited unknown note");
        let provenance = archive.notes[0].provenance.as_ref().expect("provenance");
        assert!(provenance.review_tracking_known);
        assert_eq!(provenance.reviewed_revision, None);
    }

    #[test]
    fn an_edited_acl_advances_the_revision() {
        let note = reviewed_note("0197c9bc-0000-7000-8000-000000000001", "題名", "alice");
        let snapshot = LogicalSnapshot::new(
            vec![note.clone()],
            vec![marginalis_application::NoteAclSnapshotEntry::new(
                note.note_id(),
                identity("bob"),
                NotePermission::Read,
            )],
        )
        .expect("snapshot");
        let export = export(&snapshot);
        let files = exported_files(&export);
        let mut changed = export.manifest.clone();
        changed.owners[0].notes[0].acl[0].permission = NotePermission::Edit;

        let archive = archive_from_documents(&changed, &files, "0.23.0", UnixMillis::new(5000))
            .expect("rebuild changed ACL");
        assert_eq!(archive.notes[0].revision, 2);
        assert_eq!(
            archive.notes[0]
                .provenance
                .as_ref()
                .and_then(|provenance| provenance.reviewed_revision),
            Some(1)
        );
    }

    #[test]
    fn a_missing_file_or_an_unknown_format_stops_the_import() {
        let snapshot = LogicalSnapshot::new(
            vec![note(
                "0197c9bc-0000-7000-8000-000000000001",
                "題名",
                "alice",
                false,
            )],
            Vec::new(),
        )
        .expect("snapshot");
        let export = export(&snapshot);
        let files = export
            .files
            .iter()
            .map(|file| (file.path.clone(), file.contents.as_bytes().to_vec()))
            .collect::<BTreeMap<_, _>>();

        let mut without_note = files.clone();
        without_note.remove(&export.manifest.owners[0].notes[0].file);
        assert_eq!(
            archive_from_documents(
                &export.manifest,
                &without_note,
                "0.23.0",
                UnixMillis::new(5000),
            ),
            Err(DocumentImportError::MissingNoteFile { position: 1 })
        );

        let mut unknown = export.manifest.clone();
        unknown.format = "marginalis-documents-99".into();
        assert_eq!(
            archive_from_documents(&unknown, &files, "0.23.0", UnixMillis::new(5000)),
            Err(DocumentImportError::UnsupportedFormat)
        );

        let mut invalid_hash = export.manifest.clone();
        invalid_hash.owners[0].notes[0].state_sha256 = "not-a-sha256".into();
        assert_eq!(
            archive_from_documents(&invalid_hash, &files, "0.23.0", UnixMillis::new(5000),),
            Err(DocumentImportError::InvalidManifestEntry { position: 1 })
        );
    }

    #[test]
    fn a_different_recorded_version_asks_for_revalidation() {
        let snapshot = LogicalSnapshot::new(Vec::new(), Vec::new()).expect("snapshot");
        let manifest = export(&snapshot).manifest;

        assert!(!requires_revalidation(&manifest, "0.23.0"));
        assert!(requires_revalidation(&manifest, "0.24.0"));
    }

    #[test]
    fn deleted_notes_are_not_written() {
        let snapshot = LogicalSnapshot::new(
            vec![
                note(
                    "0197c9bc-0000-7000-8000-000000000001",
                    "残る",
                    "alice",
                    false,
                ),
                note(
                    "0197c9bc-0000-7000-8000-000000000002",
                    "消えた",
                    "alice",
                    true,
                ),
            ],
            Vec::new(),
        )
        .expect("snapshot");

        let export = export(&snapshot);

        assert_eq!(export.manifest.owners[0].notes.len(), 1);
        assert!(
            export
                .files
                .iter()
                .all(|file| !file.path.contains("消えた"))
        );
    }

    #[test]
    fn file_names_stay_safe_and_unique() {
        let snapshot = LogicalSnapshot::new(
            vec![
                note(
                    "0197c9bc-0000-7000-8000-000000000001",
                    "../etc/passwd",
                    "alice",
                    false,
                ),
                note(
                    "0197c9bc-0000-7000-8000-000000000002",
                    "同じ題名",
                    "alice",
                    false,
                ),
                note(
                    "0197c9bc-0000-7000-8000-000000000003",
                    "同じ題名",
                    "alice",
                    false,
                ),
            ],
            Vec::new(),
        )
        .expect("snapshot");

        let files = export(&snapshot)
            .files
            .into_iter()
            .map(|file| file.path)
            .collect::<Vec<_>>();

        assert!(
            files
                .iter()
                .all(|path| !path.contains("..") && !path.contains("/etc/")),
            "{files:?}"
        );
        let notes = files
            .iter()
            .filter(|path| path.ends_with(".adoc"))
            .collect::<Vec<_>>();
        assert_eq!(notes.len(), 3);
        assert_eq!(
            notes
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            3
        );
    }

    #[test]
    fn a_title_that_leaves_no_usable_characters_falls_back_to_the_note_id() {
        let note_id = "0197c9bc-0000-7000-8000-000000000001";
        let snapshot = LogicalSnapshot::new(vec![note(note_id, "...", "alice", false)], Vec::new())
            .expect("snapshot");

        let export = export(&snapshot);

        assert_eq!(
            export.manifest.owners[0].notes[0].file,
            format!("id.example.test/alice/notes/{note_id}.adoc")
        );
    }
}
