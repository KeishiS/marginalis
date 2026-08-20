//! MCP toolの名前、入出力型、JSON Schema。

use marginalis_domain::{
    ENTITY_ID_PATTERN, NOTE_POLICY, NoteCreationSource, NoteReviewStatus, Revision,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCPクライアントが接続時に読む、toolの使い分けに関する案内。
pub const MCP_SERVER_INSTRUCTIONS: &str = concat!(
    "Use get_note_profile before creating or updating notes. ",
    "MCP note writes reject every warning-severity diagnostic; follow the profile's advisory ",
    "rules and authoring guidance before writing. ",
    "For long notes, read with get_note_outline and get_note_fragment instead of get_note, ",
    "and edit with apply_note_patch instead of resending the whole source; ",
    "replace_note_source rewrites the entire note. ",
    "Before creating a note with create_note, list templates with list_note_templates and ",
    "start from a matching template when one fits the intended content. ",
    "Bibliography bulk import is not available through MCP; add items one at a time with ",
    "add_bibliography_item."
);

use crate::{ProblemResponse, rest::nullable};

const NOTE_ID_PATTERN: &str = ENTITY_ID_PATTERN;
const MINIMUM_REVISION: i64 = Revision::MINIMUM_VALUE;
const MAX_SOURCE_BYTES: usize = NOTE_POLICY.max_source_bytes;
const MAX_PATCH_BYTES: usize = NOTE_POLICY.max_patch_bytes;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolName {
    ListNotes,
    ListNoteTemplates,
    GetNoteProfile,
    GetNote,
    GetNoteOutline,
    GetNoteFragment,
    CreateNote,
    ApplyNotePatch,
    ReplaceNoteSource,
    DeleteNote,
    SearchBibliography,
    AddBibliographyItem,
    DeleteBibliographyItem,
}

impl McpToolName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ListNotes => "list_notes",
            Self::ListNoteTemplates => "list_note_templates",
            Self::GetNoteProfile => "get_note_profile",
            Self::GetNote => "get_note",
            Self::GetNoteOutline => "get_note_outline",
            Self::GetNoteFragment => "get_note_fragment",
            Self::CreateNote => "create_note",
            Self::ApplyNotePatch => "apply_note_patch",
            Self::ReplaceNoteSource => "replace_note_source",
            Self::DeleteNote => "delete_note",
            Self::SearchBibliography => "search_bibliography",
            Self::AddBibliographyItem => "add_bibliography_item",
            Self::DeleteBibliographyItem => "delete_bibliography_item",
        }
    }

    pub const fn accepted_scopes(self) -> &'static [&'static str] {
        match self {
            Self::ListNotes
            | Self::ListNoteTemplates
            | Self::GetNote
            | Self::GetNoteOutline
            | Self::GetNoteFragment => &["notes:read"],
            Self::GetNoteProfile => &["notes:read", "notes:write"],
            Self::CreateNote | Self::ApplyNotePatch | Self::ReplaceNoteSource => &["notes:write"],
            Self::DeleteNote => &["notes:delete"],
            Self::SearchBibliography => &["bibliography:read"],
            Self::AddBibliographyItem => &["bibliography:write"],
            Self::DeleteBibliographyItem => &["bibliography:delete"],
        }
    }

    /// toolを実行するために満たす必要があるscopeの組。
    ///
    /// 外側の各要素はすべて満たす必要があり、内側の要素はいずれか一つを持てばよい。
    /// 現在は`get_note_profile`だけがreadまたはwriteのどちらでも利用できる。
    pub const fn scope_requirements(self) -> &'static [&'static [&'static str]] {
        match self {
            Self::ListNotes
            | Self::ListNoteTemplates
            | Self::GetNote
            | Self::GetNoteOutline
            | Self::GetNoteFragment => &[&["notes:read"]],
            Self::GetNoteProfile => &[&["notes:read", "notes:write"]],
            Self::CreateNote | Self::ApplyNotePatch | Self::ReplaceNoteSource => {
                &[&["notes:write"]]
            }
            Self::DeleteNote => &[&["notes:delete"]],
            Self::SearchBibliography => &[&["bibliography:read"]],
            Self::AddBibliographyItem => &[&["bibliography:write"]],
            Self::DeleteBibliographyItem => &[&["bibliography:delete"]],
        }
    }
}

impl TryFrom<&str> for McpToolName {
    type Error = UnknownMcpTool;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "list_notes" => Ok(Self::ListNotes),
            "list_note_templates" => Ok(Self::ListNoteTemplates),
            "get_note_profile" => Ok(Self::GetNoteProfile),
            "get_note" => Ok(Self::GetNote),
            "get_note_outline" => Ok(Self::GetNoteOutline),
            "get_note_fragment" => Ok(Self::GetNoteFragment),
            "create_note" => Ok(Self::CreateNote),
            "apply_note_patch" => Ok(Self::ApplyNotePatch),
            "replace_note_source" => Ok(Self::ReplaceNoteSource),
            "delete_note" => Ok(Self::DeleteNote),
            "search_bibliography" => Ok(Self::SearchBibliography),
            "add_bibliography_item" => Ok(Self::AddBibliographyItem),
            "delete_bibliography_item" => Ok(Self::DeleteBibliographyItem),
            // `update_note`は廃止した。未知のtoolとして拒否する。
            _ => Err(UnknownMcpTool),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownMcpTool;

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolContract {
    pub name: McpToolName,
    pub description: &'static str,
    pub input_schema: Value,
    pub output_schema: Value,
}

impl McpToolContract {
    /// 成功出力と失敗出力の両方を宣言したtool契約を作る。
    ///
    /// toolはどれも`isError: true`とともに[`ProblemResponse`]を返しうるため、出力schemaは
    /// 常に成功型との選択とする。これにより`docs/mcp-tools.json`が実行時の失敗応答も表し、
    /// 契約検査が差を検出できる。
    fn new<Input: JsonSchema, Output: JsonSchema>(
        name: McpToolName,
        description: &'static str,
    ) -> Self {
        let mut output_schema = schema::<McpToolOutcome<Output>>();
        output_schema
            .as_object_mut()
            .expect("generated MCP output schema is an object")
            .insert("type".into(), Value::String("object".into()));
        Self {
            name,
            description,
            input_schema: schema::<Input>(),
            output_schema,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpEmptyInput {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGetNoteInput {
    #[schemars(regex(pattern = NOTE_ID_PATTERN))]
    pub note_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpCreateNoteInput {
    #[schemars(length(max = MAX_SOURCE_BYTES))]
    pub source: String,
}

/// ノートのAsciiDoc原文全体を置き換える入力。旧`update_note`と同じ形。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpReplaceNoteSourceInput {
    #[schemars(regex(pattern = NOTE_ID_PATTERN))]
    pub note_id: String,
    #[schemars(length(max = MAX_SOURCE_BYTES))]
    pub source: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGetNoteOutlineInput {
    #[schemars(regex(pattern = NOTE_ID_PATTERN))]
    pub note_id: String,
}

/// 行範囲は両端を含む1始まり。範囲は`get_note_outline`で確認する。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGetNoteFragmentInput {
    #[schemars(regex(pattern = NOTE_ID_PATTERN))]
    pub note_id: String,
    #[schemars(range(min = 1))]
    pub start_line: usize,
    #[schemars(range(min = 1))]
    pub end_line: usize,
    /// `get_note_outline`で得たrevision。指定した場合、現在のrevisionと異なると
    /// 本文を返さず競合として拒否する。行範囲の根拠が古くないことを確かめられる。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = MINIMUM_REVISION))]
    pub expected_revision: Option<i64>,
}

/// 保存済み原文と変更内容のUnified Diff。厳密に適用され、位置の再探索は行われない。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpApplyNotePatchInput {
    #[schemars(regex(pattern = NOTE_ID_PATTERN))]
    pub note_id: String,
    /// `--- a/note.adoc`と`+++ b/note.adoc`のfile headerを持つ単一ファイルのUnified Diff。
    #[schemars(length(max = MAX_PATCH_BYTES))]
    pub patch: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub expected_revision: i64,
    /// trueでは適用と検証まで行い、保存せずrevisionも増やさない。既定はfalse。
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpDeleteNoteInput {
    #[schemars(regex(pattern = NOTE_ID_PATTERN))]
    pub note_id: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSearchBibliographyInput {
    #[schemars(length(max = 256))]
    #[serde(default)]
    pub query: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpAddBibliographyItemInput {
    /// CSL-JSON item。`id`と`type`は必須。
    pub csl_json: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpDeleteBibliographyItemInput {
    #[schemars(regex(pattern = NOTE_ID_PATTERN))]
    pub item_id: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpBibliographyItem {
    pub item_id: String,
    pub citation_key: String,
    pub csl_json: Value,
    pub updated_at_ms: i64,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpBibliographyListOutput {
    pub items: Vec<McpBibliographyItem>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpListNotesInput {
    pub created_via: Option<NoteCreationSource>,
    pub review_status: Option<NoteReviewStatus>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpListNotesOutput {
    pub notes: Vec<crate::NoteSummaryResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpListNoteTemplatesOutput {
    pub templates: Vec<crate::NoteSummaryResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpGetNoteOutput {
    pub note_id: String,
    pub title: String,
    pub source: String,
    pub tags: Vec<String>,
    pub updated_at_ms: i64,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
    pub created_via: NoteCreationSource,
    pub review_status: NoteReviewStatus,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub reviewed_revision: Option<i64>,
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub reviewed_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteRevisionOutput {
    pub note_id: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
}

/// 見出し1つと、その節が占める行範囲。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteOutlineSection {
    /// 見出しの深さ。`==`が1。
    #[schemars(range(min = 1))]
    pub level: u8,
    pub title: String,
    /// 原文に`[#id]`と明示されたアンカー。自動生成のIDは返さない。
    #[schemars(required)]
    #[schemars(transform = nullable)]
    pub anchor: Option<String>,
    #[schemars(range(min = 1))]
    pub start_line: usize,
    /// 節の末尾の行。子節を含む階層範囲で、親子の範囲は重なる。
    #[schemars(range(min = 1))]
    pub end_line: usize,
}

/// 本文を省いた文書の構成。文書題名は`title`が持ち、`sections`には含まない。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteOutlineOutput {
    pub note_id: String,
    pub title: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
    /// 原文の総行数。1始まりの最終行の番号と一致する。
    pub line_count: usize,
    pub sections: Vec<McpNoteOutlineSection>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteFragmentOutput {
    pub note_id: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
    #[schemars(range(min = 1))]
    pub start_line: usize,
    #[schemars(range(min = 1))]
    pub end_line: usize,
    /// 指定した行範囲のAsciiDoc原文。要約や変換は行わない。
    pub fragment: String,
}

/// patch適用の結果。全文は返さない。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNotePatchOutput {
    pub note_id: String,
    /// 保存後のrevision。dry runでは保存しないためnull。
    #[schemars(required)]
    #[schemars(transform = nullable)]
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: Option<i64>,
    pub dry_run: bool,
    pub hunks_applied: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
    /// 保存を拒否しない診断。warningがあれば適用自体が失敗として返る。
    pub diagnostics: Vec<crate::NoteDiagnosticResponse>,
}

/// tool呼び出しの結果。成功出力か、共通の失敗表現のいずれかになる。
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(untagged)]
pub enum McpToolOutcome<Output> {
    Success(Output),
    Failure(ProblemResponse),
}

/// ノートの作成・更新結果。
pub type McpNoteMutationOutput = McpToolOutcome<McpNoteRevisionOutput>;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteProfileOutput {
    pub profile_version: u32,
    pub adocweave_package_version: String,
    /// このサーバーのMarginalisの版。tool契約は版と同時にしか変わらないため、
    /// クライアントが手元のtool一覧の世代を照合する識別子として使える。
    pub marginalis_version: String,
    pub limits: McpNoteProfileLimits,
    pub normalization: McpNoteProfileNormalization,
    pub syntax: McpNoteProfileSyntax,
    pub authoring_guidance: Vec<String>,
    pub allowed_source_languages: Vec<String>,
    pub forbidden_rules: Vec<McpNoteProfileRule>,
    pub advisory_rules: Vec<McpNoteProfileAdvisoryRule>,
    pub warnings_reject_write: bool,
    pub examples: Vec<McpNoteProfileExample>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteProfileLimits {
    pub applies_after_normalization: bool,
    pub max_title_characters: usize,
    pub max_source_bytes: usize,
    /// `apply_note_patch`が受け取るUnified DiffのUTF-8バイト数の上限。
    pub max_patch_bytes: usize,
    /// 一つのUnified Diffに含められるhunk数の上限。
    pub max_patch_hunks: usize,
    pub max_tags: usize,
    pub max_tag_characters: usize,
    pub max_attachment_bytes: usize,
    pub max_attachments_per_note: usize,
    pub max_attachment_bytes_per_note: usize,
    pub max_attachment_file_name_characters: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteProfileNormalization {
    pub title: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteProfileSyntax {
    pub common_blocks: Vec<String>,
    pub common_inlines: Vec<String>,
    pub source_language_optional: bool,
    pub allowed_math_languages: Vec<String>,
    /// 文書headerへ書ける文書属性の名前。ここに無い属性は保存が拒否される。
    pub allowed_document_attributes: Vec<String>,
    /// `:marginalis-citation-style:`へ書ける値。先頭が、属性を書かない場合の既定。
    pub allowed_citation_styles: Vec<String>,
    pub title_forbidden: Vec<String>,
    pub tag_forbidden: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteProfileRule {
    pub code: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteProfileAdvisoryRule {
    pub code: String,
    pub description: String,
    pub severity: crate::DiagnosticSeverityResponse,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteProfileExample {
    pub kind: String,
    pub description: String,
    pub body: String,
}

pub fn mcp_tool_contracts() -> Vec<McpToolContract> {
    vec![
        McpToolContract::new::<McpListNotesInput, McpListNotesOutput>(
            McpToolName::ListNotes,
            "List visible note summaries, optionally filtered by creation source and review status; requires notes:read",
        ),
        McpToolContract::new::<McpEmptyInput, McpListNoteTemplatesOutput>(
            McpToolName::ListNoteTemplates,
            "List visible notes tagged 'テンプレート' that serve as templates for new notes; read a template body with get_note; requires notes:read",
        ),
        McpToolContract::new::<McpEmptyInput, McpNoteProfileOutput>(
            McpToolName::GetNoteProfile,
            "Read the current note profile, including advisory rules and the MCP warning policy; requires notes:read or notes:write",
        ),
        McpToolContract::new::<McpGetNoteInput, McpGetNoteOutput>(
            McpToolName::GetNote,
            "Read one visible note in full; for long notes prefer get_note_outline and get_note_fragment; requires notes:read",
        ),
        McpToolContract::new::<McpGetNoteOutlineInput, McpNoteOutlineOutput>(
            McpToolName::GetNoteOutline,
            "Read the heading hierarchy and inclusive 1-based line ranges of a note without its body, to choose which range to read next; requires notes:read",
        ),
        McpToolContract::new::<McpGetNoteFragmentInput, McpNoteFragmentOutput>(
            McpToolName::GetNoteFragment,
            "Read the verbatim AsciiDoc source of an inclusive 1-based line range; pass the revision from get_note_outline as expected_revision so a concurrent update is rejected as a conflict instead of returning lines the outline no longer describes; requires notes:read",
        ),
        McpToolContract::new::<McpCreateNoteInput, McpNoteRevisionOutput>(
            McpToolName::CreateNote,
            "Create a note; before creating, call list_note_templates and reuse the structure of a matching template when one fits the intended content; requires notes:write; warnings reject the write and are returned as diagnostics",
        ),
        McpToolContract::new::<McpApplyNotePatchInput, McpNotePatchOutput>(
            McpToolName::ApplyNotePatch,
            "Apply a unified diff against a/note.adoc and b/note.adoc strictly at the expected revision; hunks must match the stored source exactly and are never fuzzed; generate the patch by saving the fetched source and the edited version byte-for-byte and running `diff -u --label a/note.adoc --label b/note.adoc current updated` (plain diff writes file names and timestamps into the header and is rejected); dry_run validates without saving; requires notes:write; warnings reject the write",
        ),
        McpToolContract::new::<McpReplaceNoteSourceInput, McpNoteRevisionOutput>(
            McpToolName::ReplaceNoteSource,
            "Replace the entire AsciiDoc source at the expected revision, for full rewrites, imports, and recovery; for small edits prefer apply_note_patch; requires notes:write; warnings reject the write",
        ),
        McpToolContract::new::<McpDeleteNoteInput, McpNoteRevisionOutput>(
            McpToolName::DeleteNote,
            "Soft-delete a note at the expected revision; requires notes:delete",
        ),
        McpToolContract::new::<McpSearchBibliographyInput, McpBibliographyListOutput>(
            McpToolName::SearchBibliography,
            "Search the current user's CSL-JSON bibliography library; requires bibliography:read",
        ),
        McpToolContract::new::<McpAddBibliographyItemInput, McpBibliographyItem>(
            McpToolName::AddBibliographyItem,
            "Add exactly one bibliography item in CSL-JSON format; requires bibliography:write; id and type are required and values are never inferred; MCP does not support bibliography bulk import",
        ),
        McpToolContract::new::<McpDeleteBibliographyItemInput, McpEmptyInput>(
            McpToolName::DeleteBibliographyItem,
            "Delete an owned bibliography item at the expected revision; requires bibliography:delete",
        ),
    ]
}

fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).expect("JSON Schema is serializable")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use marginalis_domain::NOTE_TEMPLATE_TAG;

    use super::*;

    #[test]
    fn tool_catalog_has_unique_typed_names_and_closed_input_objects() {
        let contracts = mcp_tool_contracts();
        assert_eq!(contracts.len(), 13);
        assert_eq!(
            contracts
                .iter()
                .map(|contract| contract.name)
                .collect::<HashSet<_>>()
                .len(),
            contracts.len()
        );
        for contract in contracts {
            assert_eq!(contract.input_schema["additionalProperties"], false);
            assert_eq!(
                McpToolName::try_from(contract.name.as_str()),
                Ok(contract.name)
            );
        }
    }

    #[test]
    fn bibliography_tool_and_server_instructions_explain_the_bulk_import_boundary() {
        let add = mcp_tool_contracts()
            .into_iter()
            .find(|contract| contract.name == McpToolName::AddBibliographyItem)
            .expect("add bibliography item contract");
        for guidance in [add.description, MCP_SERVER_INSTRUCTIONS] {
            assert!(guidance.contains("bulk import"));
            assert!(!guidance.contains("Web UI"));
            assert!(!guidance.contains("REST API"));
        }
        assert!(add.description.contains("exactly one bibliography item"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("add_bibliography_item"));
    }

    /// テンプレートの案内が、tool説明・server instructions・識別タグで一致していること。
    #[test]
    fn template_guidance_names_the_tag_and_the_tool() {
        let contracts = mcp_tool_contracts();
        let templates = contracts
            .iter()
            .find(|contract| contract.name == McpToolName::ListNoteTemplates)
            .expect("list_note_templates contract");
        assert!(templates.description.contains(NOTE_TEMPLATE_TAG));
        let create = contracts
            .iter()
            .find(|contract| contract.name == McpToolName::CreateNote)
            .expect("create_note contract");
        assert!(create.description.contains("list_note_templates"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("list_note_templates"));
    }

    #[test]
    fn tool_scopes_separate_notes_from_bibliography() {
        let expected = [
            (McpToolName::ListNotes, &["notes:read"][..]),
            (McpToolName::ListNoteTemplates, &["notes:read"][..]),
            (
                McpToolName::GetNoteProfile,
                &["notes:read", "notes:write"][..],
            ),
            (McpToolName::GetNote, &["notes:read"][..]),
            (McpToolName::GetNoteOutline, &["notes:read"][..]),
            (McpToolName::GetNoteFragment, &["notes:read"][..]),
            (McpToolName::CreateNote, &["notes:write"][..]),
            (McpToolName::ApplyNotePatch, &["notes:write"][..]),
            (McpToolName::ReplaceNoteSource, &["notes:write"][..]),
            (McpToolName::DeleteNote, &["notes:delete"][..]),
            (McpToolName::SearchBibliography, &["bibliography:read"][..]),
            (
                McpToolName::AddBibliographyItem,
                &["bibliography:write"][..],
            ),
            (
                McpToolName::DeleteBibliographyItem,
                &["bibliography:delete"][..],
            ),
        ];

        for (tool, scopes) in expected {
            assert_eq!(tool.accepted_scopes(), scopes, "{}のscope", tool.as_str());
            let requirements = tool.scope_requirements();
            assert!(!requirements.is_empty(), "{}のscope要件", tool.as_str());
            assert!(
                requirements
                    .iter()
                    .all(|alternatives| !alternatives.is_empty()),
                "{}のscope選択肢",
                tool.as_str()
            );
        }
    }

    /// すべてのtoolが、成功出力と共通の失敗出力を宣言することを確認する。
    ///
    /// 実行時はどのtoolも`isError: true`とともに`ProblemResponse`を返しうる。以前は
    /// 作成と更新だけがこれを宣言しており、公開schemaが実行時応答を表していなかった。
    #[test]
    fn every_tool_declares_the_shared_failure_output() {
        for contract in mcp_tool_contracts() {
            assert_eq!(
                contract.output_schema["type"],
                "object",
                "{}の出力schema",
                contract.name.as_str()
            );
            let alternatives = contract.output_schema["anyOf"]
                .as_array()
                .unwrap_or_else(|| panic!("{}の出力schemaはanyOf", contract.name.as_str()));
            assert_eq!(alternatives.len(), 2);
            assert_eq!(
                alternatives[1],
                serde_json::json!({"$ref": "#/$defs/Problem"}),
                "{}は共通の失敗出力を宣言します",
                contract.name.as_str()
            );
            assert_eq!(
                contract.output_schema["$defs"]["Problem"]["additionalProperties"],
                false
            );
        }
    }

    #[test]
    fn note_outputs_require_sync_metadata_and_reject_unknown_fields() {
        let list_schema = schema::<McpListNotesOutput>();
        let summary = &list_schema["$defs"]["NoteSummary"];
        assert_eq!(
            summary["required"],
            serde_json::json!([
                "note_id",
                "title",
                "tags",
                "updated_at_ms",
                "revision",
                "created_via",
                "review_status",
                "reviewed_revision",
                "reviewed_at_ms"
            ])
        );

        let note_schema = schema::<McpGetNoteOutput>();
        assert_eq!(
            note_schema["required"],
            serde_json::json!([
                "note_id",
                "title",
                "source",
                "tags",
                "updated_at_ms",
                "revision",
                "created_via",
                "review_status",
                "reviewed_revision",
                "reviewed_at_ms"
            ])
        );
        assert!(
            serde_json::from_value::<McpGetNoteInput>(
                serde_json::json!({"note_id": "id", "unexpected": true})
            )
            .is_err()
        );
    }

    #[test]
    fn note_profile_contract_requires_advisory_rules_and_the_warning_policy() {
        let profile_schema = schema::<McpNoteProfileOutput>();
        let required = profile_schema["required"]
            .as_array()
            .expect("profile required fields");

        assert!(required.contains(&Value::String("advisory_rules".into())));
        assert!(required.contains(&Value::String("warnings_reject_write".into())));
        assert_eq!(
            profile_schema["$defs"]["McpNoteProfileAdvisoryRule"]["required"],
            serde_json::json!(["code", "description", "severity"])
        );
    }
}
