//! MCP toolの名前、入出力型、JSON Schema。

use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const NOTE_ID_PATTERN: &str = "^[0-9a-fA-F-]{36}$";
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
        }
    }

    pub const fn accepted_scopes(self) -> &'static [&'static str] {
        match self {
            Self::ListNotes | Self::GetNote => &["notes:read"],
            Self::GetNoteProfile => &["notes:read", "notes:write"],
            Self::CreateNote | Self::UpdateNote => &["notes:write"],
            Self::DeleteNote => &["notes:delete"],
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
    fn new<Input: JsonSchema, Output: JsonSchema>(
        name: McpToolName,
        description: &'static str,
    ) -> Self {
        Self {
            name,
            description,
            input_schema: schema::<Input>(),
            output_schema: schema::<Output>(),
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
    #[schemars(range(min = 1))]
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpDeleteNoteInput {
    #[schemars(regex(pattern = NOTE_ID_PATTERN))]
    pub note_id: String,
    #[schemars(range(min = 1))]
    pub expected_revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteSummary {
    pub note_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub updated_at_ms: i64,
    #[schemars(range(min = 1))]
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
    #[schemars(range(min = 1))]
    pub revision: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpNoteRevisionOutput {
    pub note_id: String,
    #[schemars(range(min = 1))]
    pub revision: i64,
}

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
            "Create a note",
        ),
        McpToolContract::new::<McpUpdateNoteInput, McpNoteRevisionOutput>(
            McpToolName::UpdateNote,
            "Update a note at the expected revision",
        ),
        McpToolContract::new::<McpDeleteNoteInput, McpNoteRevisionOutput>(
            McpToolName::DeleteNote,
            "Soft-delete a note at the expected revision",
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
        assert_eq!(contracts.len(), 6);
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
            assert_eq!(contract.output_schema["additionalProperties"], false);
            assert_eq!(
                McpToolName::try_from(contract.name.as_str()),
                Ok(contract.name)
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
