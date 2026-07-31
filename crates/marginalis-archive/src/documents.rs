//! ノートと書誌情報を、他の道具でそのまま読める形へ書き出す一方向の出力。
//!
//! ノートは保存しているAsciiDocのまま、書誌情報はCSL-JSONの配列として並べます。復元へ使う
//! [`Archive`](crate::Archive)とは目的が異なり、この出力を取り込む経路は現在ありません。
//! ただしmanifestは、archiveと同じ意味の版情報を持ちます。取り込み側は、稼働している
//! serviceの版と比べて再検証や移行が必要かどうかを判断できます。

use std::collections::BTreeMap;

use marginalis_application::LogicalSnapshot;
use marginalis_domain::{Identity, Note, NotePermission, UnixMillis};
use serde::{Deserialize, Serialize};

/// この出力の形式名。archiveの版とは別に管理する。
pub const DOCUMENT_EXPORT_FORMAT: &str = "marginalis-documents-1";

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
    /// この出力を取り込む経路がないことを、読み手へ明示する。
    pub restore_source: &'static str,
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
        restore_source: RESTORE_SOURCE,
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
                DocumentNote {
                    file,
                    note_id: note.note_id().to_string(),
                    title: note.title().to_owned(),
                    tags: note.tags().to_vec(),
                    created_at_ms: note.created_at().get(),
                    updated_at_ms: note.updated_at().get(),
                    revision: note.revision().get(),
                    acl: snapshot
                        .note_acl()
                        .iter()
                        .filter(|entry| entry.note_id() == note.note_id())
                        .map(|entry| DocumentAclEntry {
                            issuer: entry.identity().issuer().to_owned(),
                            subject: entry.identity().subject().to_owned(),
                            permission: entry.permission(),
                        })
                        .collect(),
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use marginalis_domain::{
        BibliographyItem, BibliographyItemId, EntityId, NoteId, Revision, UnixMillis,
    };

    use super::*;

    fn identity(subject: &str) -> Identity {
        Identity::new("https://id.example.test".into(), subject.into()).expect("owner")
    }

    fn note(id: &str, title: &str, owner: &str, deleted: bool) -> Note {
        Note::restore(
            NoteId::new(EntityId::from_str(id).expect("UUIDv7")),
            identity(owner),
            title.into(),
            format!("= {title}\n\n本文"),
            vec!["研究".into()],
            UnixMillis::new(1000),
            UnixMillis::new(2000),
            Revision::INITIAL,
            deleted.then(|| UnixMillis::new(2000)),
        )
        .expect("note")
    }

    fn item(citation_key: &str, owner: &str) -> BibliographyItem {
        BibliographyItem::create(
            BibliographyItemId::new(
                EntityId::from_str("0197c9bc-0000-7000-8000-0000000000a1").expect("UUIDv7"),
            ),
            &identity(owner),
            citation_key.into(),
            serde_json::json!({ "id": citation_key, "type": "book", "title": "Example" })
                .to_string(),
            UnixMillis::new(1000),
        )
    }

    fn export(snapshot: &LogicalSnapshot) -> DocumentExport {
        create_document_export(snapshot, "0.19.0", "0.23.0", UnixMillis::new(4000))
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
        .with_bibliography(vec![item("smith2024", "alice")])
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
        // 書誌情報を持たない所有者にも空の配列を出し、ファイルの有無で意味が変わらないようにする。
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
        .with_bibliography(vec![item("smith2024", "alice")])
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
