//! MCP toolの名前、入出力型、JSON Schema。

use marginalis_domain::{ENTITY_ID_PATTERN, Revision};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ProblemResponse;

const NOTE_ID_PATTERN: &str = ENTITY_ID_PATTERN;
const MINIMUM_REVISION: i64 = Revision::MINIMUM_VALUE;
const MAX_SOURCE_BYTES: usize = 524_288;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolName {
    ListNotes,
    GetNoteProfile,
    GetNote,
    CreateNote,
    UpdateNote,
    DeleteNote,
    SearchBibliography,
    AddBibliographyItem,
    AddBibliographyItems,
    DeleteBibliographyItem,
}

impl McpToolName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ListNotes => "list_notes",
            Self::GetNoteProfile => "get_note_profile",
            Self::GetNote => "get_note",
            Self::CreateNote => "create_note",
            Self::UpdateNote => "update_note",
            Self::DeleteNote => "delete_note",
            Self::SearchBibliography => "search_bibliography",
            Self::AddBibliographyItem => "add_bibliography_item",
            Self::AddBibliographyItems => "add_bibliography_items",
            Self::DeleteBibliographyItem => "delete_bibliography_item",
        }
    }

    pub const fn accepted_scopes(self) -> &'static [&'static str] {
        match self {
            Self::ListNotes | Self::GetNote | Self::SearchBibliography => &["notes:read"],
            Self::GetNoteProfile => &["notes:read", "notes:write"],
            Self::CreateNote
            | Self::UpdateNote
            | Self::AddBibliographyItem
            | Self::AddBibliographyItems => &["notes:write"],
            Self::DeleteNote | Self::DeleteBibliographyItem => &["notes:delete"],
        }
    }
}

impl TryFrom<&str> for McpToolName {
    type Error = UnknownMcpTool;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "list_notes" => Ok(Self::ListNotes),
            "get_note_profile" => Ok(Self::GetNoteProfile),
            "get_note" => Ok(Self::GetNote),
            "create_note" => Ok(Self::CreateNote),
            "update_note" => Ok(Self::UpdateNote),
            "delete_note" => Ok(Self::DeleteNote),
            "search_bibliography" => Ok(Self::SearchBibliography),
            "add_bibliography_item" => Ok(Self::AddBibliographyItem),
            "add_bibliography_items" => Ok(Self::AddBibliographyItems),
            "delete_bibliography_item" => Ok(Self::DeleteBibliographyItem),
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

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpUpdateNoteInput {
    #[schemars(regex(pattern = NOTE_ID_PATTERN))]
    pub note_id: String,
    #[schemars(length(max = MAX_SOURCE_BYTES))]
    pub source: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub expected_revision: i64,
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
pub struct McpAddBibliographyItemsInput {
    /// CSL-JSON items。各項目の`id`と`type`は必須。
    #[schemars(length(min = 1, max = 100))]
    pub csl_json_items: Vec<Value>,
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

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpBibliographyImportError {
    pub input_index: usize,
    pub citation_key: Option<String>,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpBibliographyImportOutput {
    pub items: Vec<McpBibliographyItem>,
    pub errors: Vec<McpBibliographyImportError>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteSummary {
    pub note_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at_ms: i64,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpListNotesOutput {
    pub notes: Vec<McpNoteSummary>,
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
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteRevisionOutput {
    pub note_id: String,
    #[schemars(range(min = MINIMUM_REVISION))]
    pub revision: i64,
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
    pub limits: McpNoteProfileLimits,
    pub normalization: McpNoteProfileNormalization,
    pub syntax: McpNoteProfileSyntax,
    pub authoring_guidance: Vec<String>,
    pub allowed_source_languages: Vec<String>,
    pub forbidden_rules: Vec<McpNoteProfileRule>,
    pub examples: Vec<McpNoteProfileExample>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteProfileLimits {
    pub applies_after_normalization: bool,
    pub max_title_characters: usize,
    pub max_source_bytes: usize,
    pub max_tags: usize,
    pub max_tag_characters: usize,
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
pub struct McpNoteProfileExample {
    pub kind: String,
    pub description: String,
    pub body: String,
}

pub fn mcp_tool_contracts() -> Vec<McpToolContract> {
    vec![
        McpToolContract::new::<McpEmptyInput, McpListNotesOutput>(
            McpToolName::ListNotes,
            "List visible note summaries",
        ),
        McpToolContract::new::<McpEmptyInput, McpNoteProfileOutput>(
            McpToolName::GetNoteProfile,
            "Read the current note profile",
        ),
        McpToolContract::new::<McpGetNoteInput, McpGetNoteOutput>(
            McpToolName::GetNote,
            "Read one visible note",
        ),
        McpToolContract::new::<McpCreateNoteInput, McpNoteRevisionOutput>(
            McpToolName::CreateNote,
            "Create a note; warnings reject the write and are returned as diagnostics",
        ),
        McpToolContract::new::<McpUpdateNoteInput, McpNoteRevisionOutput>(
            McpToolName::UpdateNote,
            "Update a note at the expected revision; warnings reject the write and are returned as diagnostics",
        ),
        McpToolContract::new::<McpDeleteNoteInput, McpNoteRevisionOutput>(
            McpToolName::DeleteNote,
            "Soft-delete a note at the expected revision",
        ),
        McpToolContract::new::<McpSearchBibliographyInput, McpBibliographyListOutput>(
            McpToolName::SearchBibliography,
            "Search the current user's CSL-JSON bibliography library",
        ),
        McpToolContract::new::<McpAddBibliographyItemInput, McpBibliographyItem>(
            McpToolName::AddBibliographyItem,
            "Add one bibliography item in CSL-JSON format; id and type are required and values are never inferred",
        ),
        McpToolContract::new::<McpAddBibliographyItemsInput, McpBibliographyImportOutput>(
            McpToolName::AddBibliographyItems,
            "Add multiple bibliography items in CSL-JSON format; successful items and per-input errors are returned",
        ),
        McpToolContract::new::<McpDeleteBibliographyItemInput, McpEmptyInput>(
            McpToolName::DeleteBibliographyItem,
            "Delete an owned bibliography item at the expected revision",
        ),
    ]
}

fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).expect("JSON Schema is serializable")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn tool_catalog_has_unique_typed_names_and_closed_input_objects() {
        let contracts = mcp_tool_contracts();
        assert_eq!(contracts.len(), 10);
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
                serde_json::json!({"$ref": "#/$defs/ProblemResponse"}),
                "{}は共通の失敗出力を宣言します",
                contract.name.as_str()
            );
            assert_eq!(
                contract.output_schema["$defs"]["ProblemResponse"]["additionalProperties"],
                false
            );
        }
    }

    #[test]
    fn note_outputs_require_sync_metadata_and_reject_unknown_fields() {
        let list_schema = schema::<McpListNotesOutput>();
        let summary = &list_schema["$defs"]["McpNoteSummary"];
        assert_eq!(
            summary["required"],
            serde_json::json!(["note_id", "title", "tags", "updated_at_ms", "revision"])
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
                "revision"
            ])
        );
        assert!(
            serde_json::from_value::<McpGetNoteInput>(
                serde_json::json!({"note_id": "id", "unexpected": true})
            )
            .is_err()
        );
    }
}
