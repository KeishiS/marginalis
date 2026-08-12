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
    "Bibliography bulk import is not available through MCP: add items one at a time with ",
    "add_bibliography_item, or use the Web UI or REST API for file preview and conflict resolution."
);

use crate::{ProblemResponse, rest::nullable};

const NOTE_ID_PATTERN: &str = ENTITY_ID_PATTERN;
const MINIMUM_REVISION: i64 = Revision::MINIMUM_VALUE;
const MAX_SOURCE_BYTES: usize = NOTE_POLICY.max_source_bytes;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolName {
    ListNotes,
    SyncNotes,
    GetNoteProfile,
    GetNote,
    CreateNote,
    UpdateNote,
    DeleteNote,
    SearchBibliography,
    AddBibliographyItem,
    DeleteBibliographyItem,
}

impl McpToolName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ListNotes => "list_notes",
            Self::SyncNotes => "sync_notes",
            Self::GetNoteProfile => "get_note_profile",
            Self::GetNote => "get_note",
            Self::CreateNote => "create_note",
            Self::UpdateNote => "update_note",
            Self::DeleteNote => "delete_note",
            Self::SearchBibliography => "search_bibliography",
            Self::AddBibliographyItem => "add_bibliography_item",
            Self::DeleteBibliographyItem => "delete_bibliography_item",
        }
    }

    pub const fn accepted_scopes(self) -> &'static [&'static str] {
        match self {
            Self::ListNotes | Self::GetNote => &["notes:read"],
            Self::SyncNotes => &["notes:sync"],
            Self::GetNoteProfile => &["notes:read", "notes:write"],
            Self::CreateNote | Self::UpdateNote => &["notes:write"],
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
            Self::ListNotes | Self::GetNote => &[&["notes:read"]],
            Self::SyncNotes => &[&["notes:sync"]],
            Self::GetNoteProfile => &[&["notes:read", "notes:write"]],
            Self::CreateNote | Self::UpdateNote => &[&["notes:write"]],
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
            "sync_notes" => Ok(Self::SyncNotes),
            "get_note_profile" => Ok(Self::GetNoteProfile),
            "get_note" => Ok(Self::GetNote),
            "create_note" => Ok(Self::CreateNote),
            "update_note" => Ok(Self::UpdateNote),
            "delete_note" => Ok(Self::DeleteNote),
            "search_bibliography" => Ok(Self::SearchBibliography),
            "add_bibliography_item" => Ok(Self::AddBibliographyItem),
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

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSyncNotesInput {
    pub cursor: Option<String>,
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<usize>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpSyncPhase {
    Snapshot,
    Changes,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpSyncRemovalReason {
    Deleted,
    AccessRevoked,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpSyncEntry {
    Upsert {
        note: McpGetNoteOutput,
    },
    Remove {
        note_id: String,
        reason: McpSyncRemovalReason,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSyncNotesOutput {
    pub phase: McpSyncPhase,
    pub entries: Vec<McpSyncEntry>,
    pub next_cursor: String,
    pub has_more: bool,
    pub cursor_expires_at_ms: i64,
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
        McpToolContract::new::<McpSyncNotesInput, McpSyncNotesOutput>(
            McpToolName::SyncNotes,
            "Synchronize a persistent search projection with a snapshot followed by changes; requires notes:sync",
        ),
        McpToolContract::new::<McpEmptyInput, McpNoteProfileOutput>(
            McpToolName::GetNoteProfile,
            "Read the current note profile; requires notes:read or notes:write",
        ),
        McpToolContract::new::<McpGetNoteInput, McpGetNoteOutput>(
            McpToolName::GetNote,
            "Read one visible note; requires notes:read",
        ),
        McpToolContract::new::<McpCreateNoteInput, McpNoteRevisionOutput>(
            McpToolName::CreateNote,
            "Create a note; requires notes:write; warnings reject the write and are returned as diagnostics",
        ),
        McpToolContract::new::<McpUpdateNoteInput, McpNoteRevisionOutput>(
            McpToolName::UpdateNote,
            "Update a note at the expected revision; requires notes:write; warnings reject the write and are returned as diagnostics",
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
            "Add exactly one bibliography item in CSL-JSON format; requires bibliography:write; id and type are required and values are never inferred; MCP does not support bibliography bulk import, so use the Web UI or REST API for file preview and conflict resolution",
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

    #[test]
    fn bibliography_tool_and_server_instructions_explain_the_bulk_import_boundary() {
        let add = mcp_tool_contracts()
            .into_iter()
            .find(|contract| contract.name == McpToolName::AddBibliographyItem)
            .expect("add bibliography item contract");
        for guidance in [add.description, MCP_SERVER_INSTRUCTIONS] {
            assert!(guidance.contains("bulk import"));
            assert!(guidance.contains("Web UI or REST API"));
        }
        assert!(add.description.contains("exactly one bibliography item"));
        assert!(MCP_SERVER_INSTRUCTIONS.contains("add_bibliography_item"));
    }

    #[test]
    fn tool_scopes_separate_notes_from_bibliography() {
        let expected = [
            (McpToolName::ListNotes, &["notes:read"][..]),
            (McpToolName::SyncNotes, &["notes:sync"][..]),
            (
                McpToolName::GetNoteProfile,
                &["notes:read", "notes:write"][..],
            ),
            (McpToolName::GetNote, &["notes:read"][..]),
            (McpToolName::CreateNote, &["notes:write"][..]),
            (McpToolName::UpdateNote, &["notes:write"][..]),
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
}
