//! 本アプリ向けのAdocWeave統合境界。
//!
//! このcrateはAdocWeaveの公開APIだけに依存し、アプリ固有のプロファイル、参照解決、
//! 描画ポリシーを段階的に追加する。

use core::fmt;
use std::collections::BTreeMap;

use adocweave::attributes::{AttributeOperation, DocumentAttribute};
use adocweave::limits::SyntaxMode;
use adocweave::parser::{AstBlock, HeadingKind};
use adocweave::source::{TextRange, TextSize};
use unicode_normalization::UnicodeNormalization;

/// 採用したAdocWeaveソースcommit。
pub const ADOCWEAVE_SOURCE_REVISION: &str = "72ad5a677e179448b4de7f524710f4e455aa163d";

/// アプリが受理するAdocWeave公開契約の組。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdocWeaveContracts {
    pub core_profile: u16,
    pub core_api: u16,
    pub html: u16,
    pub projection: u16,
    pub conformance: u16,
    pub wasm_api: u16,
}

/// 現在固定しているAdocWeaveリリース契約。
pub const PINNED_CONTRACTS: AdocWeaveContracts = AdocWeaveContracts {
    core_profile: 1,
    core_api: 1,
    html: 1,
    projection: 1,
    conformance: 1,
    wasm_api: 1,
};

/// 実際にlinkされたAdocWeaveの契約。
pub const fn runtime_contracts() -> AdocWeaveContracts {
    AdocWeaveContracts {
        core_profile: adocweave::CORE_PROFILE_VERSION,
        core_api: adocweave::CORE_API_VERSION,
        html: adocweave::html::HTML_CONTRACT_VERSION,
        projection: adocweave::projection::PROJECTION_CONTRACT_VERSION,
        conformance: adocweave::conformance::CONFORMANCE_CONTRACT_VERSION,
        wasm_api: adocweave_wasm::WASM_API_VERSION,
    }
}

/// 固定した契約と実行時の契約が異なる場合に返すエラー。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContractMismatch {
    pub expected: AdocWeaveContracts,
    pub actual: AdocWeaveContracts,
}

impl fmt::Display for ContractMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "AdocWeave contract mismatch: expected {:?}, got {:?}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for ContractMismatch {}

/// linkされた依存が、本アプリの固定した公開契約と一致することを検証する。
pub fn verify_runtime_contracts() -> Result<(), ContractMismatch> {
    let actual = runtime_contracts();
    if actual == PINNED_CONTRACTS {
        Ok(())
    } else {
        Err(ContractMismatch {
            expected: PINNED_CONTRACTS,
            actual,
        })
    }
}

/// 保存済みノートで変更を許可しない文書属性。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImmutableNoteMetadata {
    pub note_id: String,
    pub creator_id: String,
    pub created_at: String,
}

impl ImmutableNoteMetadata {
    /// 既存ノートを解析するためのAdocWeave設定を作る。
    pub fn parse_options(&self, syntax_mode: SyntaxMode) -> adocweave::ParseOptions {
        let protected_attributes = BTreeMap::from([
            ("note-id".to_owned(), self.note_id.clone()),
            ("creator-id".to_owned(), self.creator_id.clone()),
            ("created-at".to_owned(), self.created_at.clone()),
        ]);
        adocweave::ParseOptions {
            syntax_mode,
            protected_attributes,
            ..adocweave::ParseOptions::default()
        }
    }
}

/// 正規化済みタグ。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteTag {
    /// 利用者が指定した表示用の綴りをUnicode NFCで正規化した値。
    pub display: String,
    /// 重複排除とソートに使うロケール非依存のキー。
    pub key: String,
}

/// ノートヘッダから抽出したアプリ固有メタデータ。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteMetadata {
    pub title: String,
    pub note_id: String,
    pub creator_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub tags: Vec<NoteTag>,
}

/// ノート用属性検証が返す、位置付きの安定したエラー。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProfileError {
    pub code: NoteProfileErrorCode,
    pub range: TextRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteProfileErrorCode {
    MissingAttribute,
    DuplicateAttribute,
    UnsetAttribute,
    InvalidNoteId,
    InvalidCreatorId,
    InvalidCreatedAt,
    InvalidUpdatedAt,
    InvalidTags,
    TooManyTags,
    TagTooLong,
}

impl NoteProfileErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingAttribute => "missing-note-attribute",
            Self::DuplicateAttribute => "duplicate-note-attribute",
            Self::UnsetAttribute => "unset-note-attribute",
            Self::InvalidNoteId => "invalid-note-id",
            Self::InvalidCreatorId => "invalid-creator-id",
            Self::InvalidCreatedAt => "invalid-created-at",
            Self::InvalidUpdatedAt => "invalid-updated-at",
            Self::InvalidTags => "invalid-tags",
            Self::TooManyTags => "too-many-tags",
            Self::TagTooLong => "tag-too-long",
        }
    }
}

/// AdocWeaveの標準属性から本アプリのノートメタデータを抽出・検証する。
///
/// 必須属性を重複またはunsetで曖昧にしない。属性の書換えは行わず、タグだけは
/// 表示用のUnicode NFC値と照合用の正規化キーを返す。
pub fn validate_note_metadata(
    analysis: &adocweave::Analysis,
) -> Result<NoteMetadata, Vec<NoteProfileError>> {
    let title = analysis
        .ast()
        .blocks()
        .iter()
        .find_map(|block| match block {
            AstBlock::Heading(heading) if heading.kind == HeadingKind::DocumentTitle => {
                Some(heading.text.clone())
            }
            _ => None,
        })
        .unwrap_or_default();
    let mut errors = Vec::new();

    let note_id = required_attribute(analysis.ast().attributes(), "note-id", &mut errors);
    let creator_id = required_attribute(analysis.ast().attributes(), "creator-id", &mut errors);
    let created_at = required_attribute(analysis.ast().attributes(), "created-at", &mut errors);
    let updated_at = required_attribute(analysis.ast().attributes(), "updated-at", &mut errors);
    let tags = required_attribute(analysis.ast().attributes(), "tags", &mut errors);

    if let Some(attribute) = note_id {
        if !is_uuid_v7(&attribute.raw_value) {
            errors.push(NoteProfileError {
                code: NoteProfileErrorCode::InvalidNoteId,
                range: attribute.value_range,
            });
        }
    }
    if let Some(attribute) = creator_id {
        if !is_uuid_v7(&attribute.raw_value) {
            errors.push(NoteProfileError {
                code: NoteProfileErrorCode::InvalidCreatorId,
                range: attribute.value_range,
            });
        }
    }
    if let Some(attribute) = created_at {
        if !is_fixed_millisecond_timestamp(&attribute.raw_value) {
            errors.push(NoteProfileError {
                code: NoteProfileErrorCode::InvalidCreatedAt,
                range: attribute.value_range,
            });
        }
    }
    if let Some(attribute) = updated_at {
        if !is_fixed_millisecond_timestamp(&attribute.raw_value) {
            errors.push(NoteProfileError {
                code: NoteProfileErrorCode::InvalidUpdatedAt,
                range: attribute.value_range,
            });
        }
    }

    let normalized_tags = tags.and_then(|attribute| normalize_tags(attribute, &mut errors));
    errors.sort_by_key(|error| (error.range.start(), error.range.end(), error.code.as_str()));
    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(NoteMetadata {
        title,
        note_id: note_id
            .expect("validated required attribute")
            .raw_value
            .clone(),
        creator_id: creator_id
            .expect("validated required attribute")
            .raw_value
            .clone(),
        created_at: created_at
            .expect("validated required attribute")
            .raw_value
            .clone(),
        updated_at: updated_at
            .expect("validated required attribute")
            .raw_value
            .clone(),
        tags: normalized_tags.expect("validated required attribute"),
    })
}

fn required_attribute<'a>(
    attributes: &'a [DocumentAttribute],
    name: &str,
    errors: &mut Vec<NoteProfileError>,
) -> Option<&'a DocumentAttribute> {
    let matching = attributes
        .iter()
        .filter(|attribute| attribute.name == name)
        .collect::<Vec<_>>();
    let Some(attribute) = matching.first().copied() else {
        errors.push(NoteProfileError {
            code: NoteProfileErrorCode::MissingAttribute,
            range: TextRange::new(TextSize::ZERO, TextSize::ZERO).expect("empty range"),
        });
        return None;
    };

    if matching.len() > 1 {
        for duplicate in matching {
            errors.push(NoteProfileError {
                code: NoteProfileErrorCode::DuplicateAttribute,
                range: duplicate.name_range,
            });
        }
        return None;
    }
    if attribute.operation == AttributeOperation::Unset {
        errors.push(NoteProfileError {
            code: NoteProfileErrorCode::UnsetAttribute,
            range: attribute.name_range,
        });
        return None;
    }
    Some(attribute)
}

fn normalize_tags(
    attribute: &DocumentAttribute,
    errors: &mut Vec<NoteProfileError>,
) -> Option<Vec<NoteTag>> {
    if attribute.raw_value.is_empty() {
        return Some(Vec::new());
    }

    let mut tags = Vec::new();
    for raw_tag in attribute.raw_value.split(',') {
        let display = raw_tag.trim().nfc().collect::<String>();
        if display.is_empty() || display.contains(['\n', '\r']) {
            errors.push(NoteProfileError {
                code: NoteProfileErrorCode::InvalidTags,
                range: attribute.value_range,
            });
            return None;
        }
        if display.chars().count() > 64 {
            errors.push(NoteProfileError {
                code: NoteProfileErrorCode::TagTooLong,
                range: attribute.value_range,
            });
            return None;
        }
        let key = display
            .nfc()
            .flat_map(char::to_lowercase)
            .collect::<String>();
        tags.push(NoteTag { display, key });
    }
    tags.sort_by(|left, right| left.key.cmp(&right.key));
    tags.dedup_by(|left, right| left.key == right.key);
    if tags.len() > 50 {
        errors.push(NoteProfileError {
            code: NoteProfileErrorCode::TooManyTags,
            range: attribute.value_range,
        });
        return None;
    }
    Some(tags)
}

fn is_uuid_v7(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
        && bytes[14] == b'7'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B')
}

fn is_fixed_millisecond_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    const SEPARATORS: &[(usize, u8)] = &[
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'.'),
        (23, b'Z'),
    ];
    if bytes.len() != 24
        || SEPARATORS
            .iter()
            .any(|(offset, expected)| bytes[*offset] != *expected)
        || bytes
            .iter()
            .enumerate()
            .filter(|(offset, _)| !SEPARATORS.iter().any(|(separator, _)| separator == offset))
            .any(|(_, byte)| !byte.is_ascii_digit())
    {
        return false;
    }

    let year = four_digits(bytes, 0);
    let month = two_digits(bytes, 5);
    let day = two_digits(bytes, 8);
    let hour = two_digits(bytes, 11);
    let minute = two_digits(bytes, 14);
    let second = two_digits(bytes, 17);
    (1..=12).contains(&month)
        && day >= 1
        && day <= days_in_month(year, month)
        && hour <= 23
        && minute <= 59
        && second <= 59
}

fn two_digits(bytes: &[u8], start: usize) -> u16 {
    u16::from(bytes[start] - b'0') * 10 + u16::from(bytes[start + 1] - b'0')
}

fn four_digits(bytes: &[u8], start: usize) -> u16 {
    u16::from(bytes[start] - b'0') * 1000
        + u16::from(bytes[start + 1] - b'0') * 100
        + u16::from(bytes[start + 2] - b'0') * 10
        + u16::from(bytes[start + 3] - b'0')
}

fn days_in_month(year: u16, month: u16) -> u16 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use adocweave::Engine;

    use super::{
        ADOCWEAVE_SOURCE_REVISION, NoteProfileErrorCode, PINNED_CONTRACTS, validate_note_metadata,
        verify_runtime_contracts,
    };

    #[test]
    fn pinned_revision_is_a_full_git_object_id() {
        assert_eq!(ADOCWEAVE_SOURCE_REVISION.len(), 40);
        assert!(
            ADOCWEAVE_SOURCE_REVISION
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
    }

    #[test]
    fn linked_contracts_match_the_pinned_contracts() {
        assert_eq!(PINNED_CONTRACTS.core_profile, 1);
        verify_runtime_contracts().expect("pinned AdocWeave contracts must match");
    }

    #[test]
    fn validates_metadata_and_normalizes_duplicate_tags() {
        let analysis = Engine::new(Default::default())
            .analyze(
                "= 研究ノート\n\
                 :note-id: 01800000-0000-7000-8000-000000000001\n\
                 :creator-id: 01800000-0000-7000-8000-000000000002\n\
                 :created-at: 2026-07-21T00:00:00.000Z\n\
                 :updated-at: 2026-07-22T01:02:03.004Z\n\
                 :tags: Research, research, 数学\n\n\
                 本文。\n",
            )
            .expect("valid AsciiDoc");

        let metadata = validate_note_metadata(&analysis).expect("valid note metadata");
        assert_eq!(metadata.title, "研究ノート");
        assert_eq!(metadata.tags.len(), 2);
        assert_eq!(metadata.tags[0].display, "Research");
        assert_eq!(metadata.tags[0].key, "research");
        assert_eq!(metadata.tags[1].display, "数学");
    }

    #[test]
    fn rejects_duplicate_or_invalid_required_attributes() {
        let analysis = Engine::new(Default::default())
            .analyze(
                "= Title\n\
                 :note-id: 01800000-0000-7000-8000-000000000001\n\
                 :note-id: 01800000-0000-7000-8000-000000000001\n\
                 :creator-id: invalid\n\
                 :created-at: 2026-02-29T00:00:00.000Z\n\
                 :updated-at: 2026-02-30T00:00:00.000Z\n\
                 :tags: alpha,,beta\n",
            )
            .expect("recoverable AsciiDoc");

        let errors = validate_note_metadata(&analysis).expect_err("metadata must be rejected");
        let codes = errors
            .into_iter()
            .map(|error| error.code)
            .collect::<Vec<_>>();
        assert!(codes.contains(&NoteProfileErrorCode::DuplicateAttribute));
        assert!(codes.contains(&NoteProfileErrorCode::InvalidCreatorId));
        assert!(codes.contains(&NoteProfileErrorCode::InvalidUpdatedAt));
        assert!(codes.contains(&NoteProfileErrorCode::InvalidTags));
    }
}
