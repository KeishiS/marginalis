//! 本アプリ向けのAdocWeave統合境界。
//!
//! このcrateはAdocWeaveの公開APIだけに依存し、アプリ固有のプロファイル、参照解決、
//! 描画ポリシーを段階的に追加する。

use core::{fmt, str::FromStr};
use std::collections::{BTreeMap, BTreeSet};

use adocweave::attributes::{AttributeOperation, DocumentAttribute};
use adocweave::html::RenderPolicy;
use adocweave::inline::{Inline, MathLanguage, ReferenceDestination};
use adocweave::limits::SyntaxMode;
use adocweave::parser::{AstBlock, DelimitedContent, HeadingKind};
use adocweave::preprocessor::discover_includes;
use adocweave::projection::{FormulaKind, project};
use adocweave::render::RenderInputs;
use adocweave::source::{TextRange, TextSize};
use adocweave::url::UrlContext;
use adocweave::walker::{SemanticNode, walk};
use marginalis_domain::{
    EntityId, NoteId, NoteProjection, NoteReference as ProjectionReference, UserId,
};
use unicode_normalization::UnicodeNormalization;

/// 採用したAdocWeaveソースcommit。
pub const ADOCWEAVE_SOURCE_REVISION: &str = "f4ef9f995b909833b43e7e33d686c4de5319165b";

/// 初期リリースでシンタックスハイライト対象として受理するsource block言語。
pub const DEFAULT_SOURCE_LANGUAGES: &[&str] = &[
    "rust",
    "typescript",
    "javascript",
    "json",
    "yaml",
    "toml",
    "bash",
    "sql",
    "text",
];

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
    core_api: 2,
    html: 2,
    projection: 2,
    conformance: 2,
    wasm_api: 2,
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
    MissingTitle,
    TitleTooLong,
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

/// 本アプリのノートを参照する、未解決のxref。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteReference {
    /// `xref:note:...[]`マクロ全体のUTF-8 byte range。
    pub range: TextRange,
    pub note_id: String,
    pub anchor: Option<String>,
    /// 明示labelがなく、Resolver由来の表示ラベルを使うべきかを示す。
    pub label_is_empty: bool,
}

/// ノートxrefに限定した位置付き診断。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteReferenceError {
    pub code: NoteReferenceErrorCode,
    pub range: TextRange,
}

/// 保存前に返す、アプリケーション層へ渡せる統一した位置付き検証エラー。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteProjectionError {
    pub code: String,
    pub range: TextRange,
}

/// 同一解析revisionから抽出したLaTeX数式。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteMathProjection {
    pub range: TextRange,
    pub content_range: TextRange,
    pub display: NoteMathDisplay,
    pub source: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteMathDisplay {
    Inline,
    Block,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteReferenceErrorCode {
    InvalidNoteId,
}

/// 保存時に拒否する、ノート本文の危険な構文。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteContentError {
    pub code: NoteContentErrorCode,
    pub range: TextRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoteContentErrorCode {
    IncludeDirective,
    InlinePassthrough,
    BlockPassthrough,
    DuplicateAnchor,
    InvalidUrlScheme,
    ResourceDisabled,
    UnsupportedMathLanguage,
    UnsupportedSourceLanguage,
}

impl NoteContentErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IncludeDirective => "include-directive-disabled",
            Self::InlinePassthrough => "inline-passthrough-disabled",
            Self::BlockPassthrough => "block-passthrough-disabled",
            Self::DuplicateAnchor => "duplicate-anchor",
            Self::InvalidUrlScheme => "invalid-url-scheme",
            Self::ResourceDisabled => "resource-disabled",
            Self::UnsupportedMathLanguage => "unsupported-math-language",
            Self::UnsupportedSourceLanguage => "unsupported-source-language",
        }
    }
}

/// ノート本文で許可する標準AsciiDoc構文のホスト側プロファイル。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteContentProfile {
    pub allowed_source_languages: BTreeSet<String>,
}

impl Default for NoteContentProfile {
    fn default() -> Self {
        Self {
            allowed_source_languages: DEFAULT_SOURCE_LANGUAGES
                .iter()
                .map(|language| (*language).to_owned())
                .collect(),
        }
    }
}

impl NoteReferenceErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidNoteId => "invalid-note-uuid",
        }
    }
}

/// 標準xrefのうち、`note:`スキームを使う参照だけを抽出する。
///
/// この関数はDB照会やACL確認を行わない。対象の実在確認、アンカー確認およびhref生成は
/// `ReferenceResolver`を実装するサーバ側で行う。
pub fn extract_note_references(
    analysis: &adocweave::Analysis,
) -> Result<Vec<NoteReference>, Vec<NoteReferenceError>> {
    let mut references = Vec::new();
    let mut errors = Vec::new();
    for reference in analysis.references() {
        let ReferenceDestination::Scheme {
            scheme,
            locator,
            locator_range,
            anchor,
            ..
        } = &reference.destination
        else {
            continue;
        };
        if !scheme.eq_ignore_ascii_case("note") {
            continue;
        }
        if !is_uuid_v7(locator) {
            errors.push(NoteReferenceError {
                code: NoteReferenceErrorCode::InvalidNoteId,
                range: *locator_range,
            });
            continue;
        }
        references.push(NoteReference {
            range: reference.range,
            note_id: locator.clone(),
            anchor: anchor.clone(),
            label_is_empty: reference.label.is_empty(),
        });
    }
    errors.sort_by_key(|error| (error.range.start(), error.range.end(), error.code.as_str()));
    if errors.is_empty() {
        Ok(references)
    } else {
        Err(errors)
    }
}

/// AdocWeave解析を、SQLite adapterが必要とする型付きノート投影へ変換する。
///
/// この境界は構文検証のみを担当し、DB検索・ACL判定・ファイルI/Oは行わない。
pub fn build_note_projection(
    analysis: &adocweave::Analysis,
) -> Result<NoteProjection, Vec<NoteProjectionError>> {
    let metadata = validate_note_metadata(analysis);
    let references = extract_note_references(analysis);
    let content_errors = validate_note_content_profile(analysis);
    let mut errors = Vec::new();
    if let Err(metadata_errors) = &metadata {
        errors.extend(metadata_errors.iter().map(|error| NoteProjectionError {
            code: error.code.as_str().into(),
            range: error.range,
        }));
    }
    if let Err(reference_errors) = &references {
        errors.extend(reference_errors.iter().map(|error| NoteProjectionError {
            code: error.code.as_str().into(),
            range: error.range,
        }));
    }
    errors.extend(content_errors.into_iter().map(|error| NoteProjectionError {
        code: error.code.as_str().into(),
        range: error.range,
    }));
    errors.sort_by(|left, right| {
        (left.range.start(), left.range.end(), &left.code).cmp(&(
            right.range.start(),
            right.range.end(),
            &right.code,
        ))
    });
    if !errors.is_empty() {
        return Err(errors);
    }
    let metadata = metadata.expect("validated metadata");
    let references = references.expect("validated references");
    let note_id = NoteId::new(EntityId::from_str(&metadata.note_id).expect("validated UUIDv7"));
    let owner_id = UserId::new(EntityId::from_str(&metadata.creator_id).expect("validated UUIDv7"));
    let projection = project(analysis, &RenderInputs::default());
    // AdocWeave標準projectionの可読テキストを正本とし、初期profileで受理するLaTeX数式を補う。
    // 生のAsciiDoc属性・マクロ記法をFTSへ流し込まない。
    let mut search_parts = vec![projection.searchable_text.text];
    search_parts.extend(
        projection
            .formulas
            .into_iter()
            .filter(|formula| formula.language == MathLanguage::Latex)
            .map(|formula| formula.source),
    );
    Ok(NoteProjection {
        note_id,
        owner_id,
        title: metadata.title,
        search_text: search_parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        anchors: analysis
            .reference_targets()
            .iter()
            .map(|target| target.id.clone())
            .collect(),
        references: references
            .into_iter()
            .map(|reference| ProjectionReference {
                source_start: reference.range.start().to_u32(),
                source_end: reference.range.end().to_u32(),
                target_note_id: reference.note_id,
                target_anchor: reference.anchor,
            })
            .collect(),
    })
}

/// UTF-8のAsciiDoc正本を解析し、保存用投影へ変換する便利なadapter入口。
pub fn parse_note_projection(source: &str) -> Result<NoteProjection, Vec<NoteProjectionError>> {
    let analysis = adocweave::Engine::new(Default::default())
        .analyze(source)
        .map_err(|_| {
            // Parser内部の詳細はtransport境界へ漏らさない。ただし空のdiagnostic集合では
            // 利用側が構文解析失敗とprofile検証失敗を区別できない。
            vec![NoteProjectionError {
                code: "asciidoc-parse-failed".into(),
                range: TextRange::new(TextSize::ZERO, TextSize::ZERO)
                    .expect("empty range is always valid"),
            }]
        })?;
    build_note_projection(&analysis)
}

/// アプリの保存プロファイルで許可しない、I/Oおよびraw HTML経路を検証する。
///
/// include検出はAdocWeaveの公開preprocessor APIを使い、ファイルやネットワークへはアクセスしない。
pub fn validate_note_content_profile(analysis: &adocweave::Analysis) -> Vec<NoteContentError> {
    validate_note_content_profile_with(analysis, &NoteContentProfile::default())
}

/// 指定したホスト側プロファイルで、I/O、raw HTMLおよび未許可の表示経路を検証する。
pub fn validate_note_content_profile_with(
    analysis: &adocweave::Analysis,
    profile: &NoteContentProfile,
) -> Vec<NoteContentError> {
    let render_policy = RenderPolicy::default();
    let mut errors = discover_includes(analysis.source())
        .expect("analysis source must have a representable byte length")
        .into_iter()
        .map(|request| NoteContentError {
            code: NoteContentErrorCode::IncludeDirective,
            range: request.range,
        })
        .collect::<Vec<_>>();
    errors.extend(
        analysis
            .resource_queries()
            .into_iter()
            .map(|query| NoteContentError {
                code: NoteContentErrorCode::ResourceDisabled,
                range: query.reference.range,
            }),
    );
    walk(analysis.ast(), |node| match node {
        SemanticNode::Inline(Inline::Passthrough { range, .. }) => errors.push(NoteContentError {
            code: NoteContentErrorCode::InlinePassthrough,
            range: *range,
        }),
        SemanticNode::Block(AstBlock::Delimited(block))
            if matches!(block.content, DelimitedContent::Passthrough(_)) =>
        {
            errors.push(NoteContentError {
                code: NoteContentErrorCode::BlockPassthrough,
                range: block.range,
            });
        }
        SemanticNode::Inline(Inline::Formula(formula))
            if formula.language != MathLanguage::Latex =>
        {
            errors.push(NoteContentError {
                code: NoteContentErrorCode::UnsupportedMathLanguage,
                range: formula.range,
            });
        }
        SemanticNode::Block(AstBlock::Math(math)) if math.language != MathLanguage::Latex => {
            errors.push(NoteContentError {
                code: NoteContentErrorCode::UnsupportedMathLanguage,
                range: math.range,
            });
        }
        SemanticNode::Block(AstBlock::Source(source)) => {
            let Some(language) = source.language.as_deref() else {
                return;
            };
            let normalized = language.to_ascii_lowercase();
            if !profile.allowed_source_languages.contains(&normalized) {
                errors.push(NoteContentError {
                    code: NoteContentErrorCode::UnsupportedSourceLanguage,
                    range: source.language_range.unwrap_or(source.attribute_range),
                });
            }
        }
        SemanticNode::Inline(Inline::Link(link))
            if !render_policy.allows_url(&link.target, UrlContext::AuthoredLink) =>
        {
            errors.push(NoteContentError {
                code: NoteContentErrorCode::InvalidUrlScheme,
                range: link.target_range,
            });
        }
        _ => {}
    });
    let mut seen_anchor_ids = BTreeSet::new();
    for target in analysis.reference_targets() {
        if !seen_anchor_ids.insert(&target.id) {
            errors.push(NoteContentError {
                code: NoteContentErrorCode::DuplicateAnchor,
                range: target.id_range,
            });
        }
    }
    errors.sort_by_key(|error| (error.range.start(), error.range.end(), error.code.as_str()));
    errors
}

/// AdocWeaveの標準projectionから、検索・安全な数式表示に使うLaTeX数式を抽出する。
pub fn extract_note_math(analysis: &adocweave::Analysis) -> Vec<NoteMathProjection> {
    project(analysis, &RenderInputs::default())
        .formulas
        .into_iter()
        .filter(|formula| formula.language == MathLanguage::Latex)
        .map(|formula| NoteMathProjection {
            range: formula.source_range,
            content_range: formula.content_range,
            display: match formula.kind {
                FormulaKind::Inline => NoteMathDisplay::Inline,
                FormulaKind::Block => NoteMathDisplay::Block,
            },
            source: formula.source,
        })
        .collect()
}

impl NoteProfileErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingTitle => "missing-note-title",
            Self::TitleTooLong => "note-title-too-long",
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
                Some((heading.text.clone(), heading.text_range))
            }
            _ => None,
        });
    let mut errors = Vec::new();

    match &title {
        None => errors.push(NoteProfileError {
            code: NoteProfileErrorCode::MissingTitle,
            range: TextRange::new(TextSize::ZERO, TextSize::ZERO).expect("empty range"),
        }),
        Some((value, range)) if value.chars().count() > 200 => errors.push(NoteProfileError {
            code: NoteProfileErrorCode::TitleTooLong,
            range: *range,
        }),
        Some(_) => {}
    }

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
        title: title.expect("validated note title").0,
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
    use adocweave::{Engine, NeverCancel, html::render};
    use adocweave_wasm::{
        WASM_API_VERSION, WasmOptions, WasmRenderInputs, WasmRequest, process_request,
    };

    use super::{
        ADOCWEAVE_SOURCE_REVISION, DEFAULT_SOURCE_LANGUAGES, NoteContentErrorCode, NoteMathDisplay,
        NoteProfileErrorCode, NoteReferenceErrorCode, PINNED_CONTRACTS, build_note_projection,
        extract_note_math, extract_note_references, validate_note_content_profile,
        validate_note_metadata, verify_runtime_contracts,
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
    fn default_wasm_rendering_matches_native_html() {
        let source = "= Preview\n\n== Section\n\nhttps://example.com[external]\n";
        let native = Engine::new(Default::default())
            .analyze(source)
            .expect("valid AsciiDoc");
        let native_html = render(native.ast(), &Default::default()).html;
        let wasm = process_request(
            WasmRequest {
                api_version: WASM_API_VERSION,
                source_id: None,
                version: 1,
                generation: 1,
                source: source.into(),
                render_inputs: WasmRenderInputs::default(),
                options: WasmOptions::default(),
            },
            &NeverCancel,
        )
        .expect("WASM request succeeds");

        assert_eq!(wasm.html, native_html);
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
    fn builds_a_typed_projection_from_validated_asciidoc() {
        let analysis = Engine::new(Default::default())
            .analyze(
                "= Typed note\n\
                 :note-id: 01800000-0000-7000-8000-000000000001\n\
                 :creator-id: 01800000-0000-7000-8000-000000000002\n\
                 :created-at: 2026-07-21T00:00:00.000Z\n\
                 :updated-at: 2026-07-22T01:02:03.004Z\n\
                 :tags: research\n\n\
                 [[start]]\n\
                 xref:note:01800000-0000-7000-8000-000000000003[target]\n",
            )
            .expect("valid AsciiDoc");
        let projection = build_note_projection(&analysis).expect("valid note projection");
        assert_eq!(projection.title, "Typed note");
        assert!(projection.search_text.contains("Typed note"));
        assert!(projection.anchors.iter().any(|anchor| anchor == "start"));
        assert_eq!(projection.references.len(), 1);
        assert!(projection.references[0].source_end > projection.references[0].source_start);
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

    #[test]
    fn rejects_missing_or_overlong_document_titles() {
        let source = format!(
            "= {}\n\
             :note-id: 01800000-0000-7000-8000-000000000001\n\
             :creator-id: 01800000-0000-7000-8000-000000000002\n\
             :created-at: 2026-07-21T00:00:00.000Z\n\
             :updated-at: 2026-07-21T00:00:00.000Z\n\
             :tags: \n",
            "題".repeat(201)
        );
        let analysis = Engine::new(Default::default())
            .analyze(&source)
            .expect("recoverable AsciiDoc");

        let errors = validate_note_metadata(&analysis).expect_err("title must be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteProfileErrorCode::TitleTooLong)
        );

        let analysis = Engine::new(Default::default())
            .analyze(":note-id: 01800000-0000-7000-8000-000000000001\n")
            .expect("recoverable AsciiDoc");
        let errors = validate_note_metadata(&analysis).expect_err("title must be required");
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteProfileErrorCode::MissingTitle)
        );
    }

    #[test]
    fn extracts_note_scheme_xrefs_without_resolving_them() {
        let analysis = Engine::new(Default::default())
            .analyze(
                "xref:note:01800000-0000-7000-8000-000000000001[]\n\n\
                 xref:note:01800000-0000-7000-8000-000000000002#stable[節]\n\n\
                 xref:other:example[別のスキーム]\n",
            )
            .expect("valid AsciiDoc");

        let references = extract_note_references(&analysis).expect("valid note references");
        assert_eq!(references.len(), 2);
        assert_eq!(
            references[0].note_id,
            "01800000-0000-7000-8000-000000000001"
        );
        assert_eq!(references[0].anchor, None);
        assert!(references[0].label_is_empty);
        assert_eq!(
            references[1].note_id,
            "01800000-0000-7000-8000-000000000002"
        );
        assert_eq!(references[1].anchor.as_deref(), Some("stable"));
        assert!(!references[1].label_is_empty);
    }

    #[test]
    fn rejects_invalid_note_uuid_without_rejecting_other_schemes() {
        let analysis = Engine::new(Default::default())
            .analyze("xref:note:not-a-uuid[不正] xref:other:not-a-uuid[許可]\n")
            .expect("recoverable AsciiDoc");

        let errors = extract_note_references(&analysis).expect_err("invalid note UUID");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, NoteReferenceErrorCode::InvalidNoteId);
    }

    #[test]
    fn rejects_include_and_passthrough_constructs() {
        let analysis = Engine::new(Default::default())
            .analyze(
                "include::secret.adoc[]\n\n\
                 +++<script>alert(1)</script>+++\n\n\
                 ++++\n<div>raw</div>\n++++\n",
            )
            .expect("recoverable AsciiDoc");

        let errors = validate_note_content_profile(&analysis);
        let codes = errors
            .into_iter()
            .map(|error| error.code)
            .collect::<Vec<_>>();
        assert!(codes.contains(&NoteContentErrorCode::IncludeDirective));
        assert!(codes.contains(&NoteContentErrorCode::InlinePassthrough));
        assert!(codes.contains(&NoteContentErrorCode::BlockPassthrough));
    }

    #[test]
    fn rejects_resource_macros() {
        let analysis = Engine::new(Default::default())
            .analyze("image::https://example.com/private.png[]\n")
            .expect("recoverable AsciiDoc");

        let errors = validate_note_content_profile(&analysis);
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteContentErrorCode::ResourceDisabled)
        );
    }

    #[test]
    fn rejects_duplicate_anchor_ids() {
        let analysis = Engine::new(Default::default())
            .analyze("[[same]]\n== First\n\n[[same]]\n== Second\n")
            .expect("recoverable AsciiDoc");

        let errors = validate_note_content_profile(&analysis);
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteContentErrorCode::DuplicateAnchor)
        );
    }

    #[test]
    fn rejects_unsafe_external_link_schemes() {
        let analysis = Engine::new(Default::default())
            .analyze("https://example.com[allowed] javascript:alert(1)[blocked]\n")
            .expect("recoverable AsciiDoc");

        let errors = validate_note_content_profile(&analysis);
        assert!(
            errors
                .iter()
                .any(|error| error.code == NoteContentErrorCode::InvalidUrlScheme)
        );
    }

    #[test]
    fn extracts_inline_and_block_latex_math() {
        let analysis = Engine::new(Default::default())
            .analyze(
                ":stem: latexmath\n\n\
                 inline stem:[x^2]\n\n\
                 [stem]\n++++\n\\sum_{i=1}^{n} i\n++++\n",
            )
            .expect("valid AsciiDoc");

        let math = extract_note_math(&analysis);
        assert_eq!(math.len(), 2);
        assert_eq!(math[0].display, NoteMathDisplay::Inline);
        assert_eq!(math[0].source, "x^2");
        assert_eq!(math[1].display, NoteMathDisplay::Block);
        assert_eq!(math[1].source, "\\sum_{i=1}^{n} i\n");
    }

    #[test]
    fn accepts_configured_source_languages_and_rejects_unknown_ones() {
        let analysis = Engine::new(Default::default())
            .analyze(
                "[source,rust]\n----\nlet value = 1;\n----\n\n\
                 [source,python]\n----\nvalue = 1\n----\n\n\
                 [source]\n----\nplain text\n----\n",
            )
            .expect("valid AsciiDoc");

        assert!(DEFAULT_SOURCE_LANGUAGES.contains(&"rust"));
        let errors = validate_note_content_profile(&analysis);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].code,
            NoteContentErrorCode::UnsupportedSourceLanguage
        );
    }
}
