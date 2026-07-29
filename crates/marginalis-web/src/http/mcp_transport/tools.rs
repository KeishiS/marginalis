//! MCP toolの入力検査、use case呼出し、契約型への変換。

use marginalis_application::{NoteProfile, NoteUseCaseError, NoteUseCases};
use marginalis_contract::{
    McpCreateNoteInput, McpDeleteNoteInput, McpGetNoteInput, McpGetNoteOutput, McpListNotesOutput,
    McpNoteProfileExample, McpNoteProfileLimits, McpNoteProfileNormalization, McpNoteProfileOutput,
    McpNoteProfileRule, McpNoteProfileSyntax, McpNoteRevisionOutput, McpNoteSummary, McpToolName,
    McpUpdateNoteInput,
};
use marginalis_domain::{Actor, Note, NoteDraft, Revision};
use serde::{Deserialize, Serialize};

use crate::mcp::JsonRpcResponse;

use super::super::{auth::parse_note_id, error::validation_problem_json};

pub(super) struct McpToolCall {
    tool: Option<McpToolName>,
    arguments: serde_json::Value,
}

impl McpToolCall {
    pub(super) fn accepted_scopes(&self) -> &'static [&'static str] {
        self.tool.map_or(&[], McpToolName::accepted_scopes)
    }
}

#[derive(Deserialize)]
struct RawMcpToolCall {
    name: String,
    #[serde(default = "empty_json_object")]
    arguments: serde_json::Value,
}

fn empty_json_object() -> serde_json::Value {
    serde_json::json!({})
}

pub(super) fn decode_tool_call(params: Option<serde_json::Value>) -> Result<McpToolCall, ()> {
    let raw =
        serde_json::from_value::<RawMcpToolCall>(params.unwrap_or_default()).map_err(|_| ())?;
    if !raw.arguments.is_object() {
        return Err(());
    }
    Ok(McpToolCall {
        tool: McpToolName::try_from(raw.name.as_str()).ok(),
        arguments: raw.arguments,
    })
}

#[derive(Serialize)]
#[serde(untagged)]
enum McpToolOutput {
    NoteList(McpListNotesOutput),
    NoteProfile(Box<McpNoteProfileOutput>),
    Note(McpGetNoteOutput),
    Revision(McpNoteRevisionOutput),
}

pub(super) async fn mcp_tool_call(
    notes: &dyn NoteUseCases,
    actor: Actor,
    id: serde_json::Value,
    call: McpToolCall,
) -> JsonRpcResponse {
    let tool = call.tool.map_or("unknown", McpToolName::as_str);
    match execute_mcp_tool(notes, actor, call).await {
        Ok(output) => {
            tracing::info!(
                event = "mcp.tool.completed",
                tool,
                outcome = "success",
                "MCP tool completed"
            );
            mcp_tool_success(id, output)
        }
        Err(failure) => {
            let outcome = failure.outcome();
            let reason = failure.reason();
            if outcome == "failure" {
                tracing::error!(
                    event = "mcp.tool.completed",
                    tool,
                    outcome,
                    reason,
                    "MCP tool failed"
                );
            } else {
                tracing::info!(
                    event = "mcp.tool.completed",
                    tool,
                    outcome,
                    reason,
                    "MCP tool was rejected"
                );
            }
            match failure {
                McpToolFailure::InvalidArguments(message) => {
                    JsonRpcResponse::error(id, -32602, message)
                }
                McpToolFailure::UnknownTool => JsonRpcResponse::error(id, -32602, "Unknown tool"),
                McpToolFailure::UseCase(error) => mcp_tool_error(id, error),
            }
        }
    }
}

enum McpToolFailure {
    InvalidArguments(&'static str),
    UnknownTool,
    UseCase(NoteUseCaseError),
}

impl McpToolFailure {
    fn outcome(&self) -> &'static str {
        match self {
            Self::UseCase(NoteUseCaseError::Unavailable) => "failure",
            Self::InvalidArguments(_)
            | Self::UnknownTool
            | Self::UseCase(
                NoteUseCaseError::Validation(_)
                | NoteUseCaseError::NotFound
                | NoteUseCaseError::Conflict
                | NoteUseCaseError::RenderFailed,
            ) => "rejected",
        }
    }

    fn reason(&self) -> &'static str {
        match self {
            Self::InvalidArguments(_) => "invalid-arguments",
            Self::UnknownTool => "unknown-tool",
            Self::UseCase(NoteUseCaseError::Validation(_)) => "validation",
            Self::UseCase(NoteUseCaseError::NotFound) => "not-found",
            Self::UseCase(NoteUseCaseError::Conflict) => "conflict",
            Self::UseCase(NoteUseCaseError::RenderFailed) => "render-failed",
            Self::UseCase(NoteUseCaseError::Unavailable) => "unavailable",
        }
    }
}

async fn execute_mcp_tool(
    notes: &dyn NoteUseCases,
    actor: Actor,
    call: McpToolCall,
) -> Result<McpToolOutput, McpToolFailure> {
    let result = match call.tool {
        Some(McpToolName::ListNotes)
            if call
                .arguments
                .as_object()
                .is_none_or(|value| !value.is_empty()) =>
        {
            return Err(McpToolFailure::InvalidArguments(
                "list arguments are invalid",
            ));
        }
        Some(McpToolName::ListNotes) => notes.list_visible_notes(actor).await.map(|notes| {
            McpToolOutput::NoteList(McpListNotesOutput {
                notes: notes
                    .into_iter()
                    .map(|entry| McpNoteSummary {
                        note_id: entry.summary.note_id.to_string(),
                        title: entry.summary.title,
                        tags: entry.summary.tags,
                        updated_at_ms: entry.summary.updated_at.get(),
                        revision: entry.summary.revision.get(),
                    })
                    .collect(),
            })
        }),
        Some(McpToolName::GetNoteProfile)
            if call
                .arguments
                .as_object()
                .is_none_or(|value| !value.is_empty()) =>
        {
            return Err(McpToolFailure::InvalidArguments(
                "profile arguments are invalid",
            ));
        }
        Some(McpToolName::GetNoteProfile) => Ok(McpToolOutput::NoteProfile(Box::new(
            note_profile_output(notes.note_profile()),
        ))),
        Some(McpToolName::GetNote) => {
            let Ok(input) = serde_json::from_value::<McpGetNoteInput>(call.arguments) else {
                return Err(McpToolFailure::InvalidArguments(
                    "get arguments are invalid",
                ));
            };
            let Some(note_id) = parse_note_id(&input.note_id).ok() else {
                return Err(McpToolFailure::InvalidArguments("note_id is invalid"));
            };
            notes.read_note(actor, note_id).await.map(|note| {
                McpToolOutput::Note(McpGetNoteOutput {
                    note_id: note.note_id().to_string(),
                    title: note.title().to_owned(),
                    source: note.source().to_owned(),
                    tags: note.tags().to_vec(),
                    updated_at_ms: note.updated_at().get(),
                    revision: note.revision().get(),
                })
            })
        }
        Some(McpToolName::CreateNote) => {
            let Ok(input) = serde_json::from_value::<McpCreateNoteInput>(call.arguments) else {
                return Err(McpToolFailure::InvalidArguments(
                    "note arguments are invalid",
                ));
            };
            notes
                .create_note(
                    actor,
                    NoteDraft {
                        source: input.source,
                        title: String::new(),
                        tags: Vec::new(),
                    },
                )
                .await
                .map(note_revision_output)
        }
        Some(McpToolName::UpdateNote) => {
            let Ok(input) = serde_json::from_value::<McpUpdateNoteInput>(call.arguments) else {
                return Err(McpToolFailure::InvalidArguments(
                    "update arguments are invalid",
                ));
            };
            let Some(note_id) = parse_note_id(&input.note_id).ok() else {
                return Err(McpToolFailure::InvalidArguments("note_id is invalid"));
            };
            let Ok(expected_revision) = Revision::new(input.expected_revision) else {
                return Err(McpToolFailure::InvalidArguments(
                    "expected_revision is invalid",
                ));
            };
            notes
                .update_note(
                    actor,
                    note_id,
                    NoteDraft {
                        source: input.source,
                        title: String::new(),
                        tags: Vec::new(),
                    },
                    expected_revision,
                )
                .await
                .map(note_revision_output)
        }
        Some(McpToolName::DeleteNote) => {
            let Ok(input) = serde_json::from_value::<McpDeleteNoteInput>(call.arguments) else {
                return Err(McpToolFailure::InvalidArguments(
                    "delete arguments are invalid",
                ));
            };
            let Some(note_id) = parse_note_id(&input.note_id).ok() else {
                return Err(McpToolFailure::InvalidArguments("note_id is invalid"));
            };
            let Ok(expected_revision) = Revision::new(input.expected_revision) else {
                return Err(McpToolFailure::InvalidArguments(
                    "expected_revision is invalid",
                ));
            };
            notes
                .soft_delete_note(actor, note_id, expected_revision)
                .await
                .map(note_revision_output)
        }
        None => return Err(McpToolFailure::UnknownTool),
    };
    result.map_err(McpToolFailure::UseCase)
}

fn mcp_tool_success(id: serde_json::Value, output: McpToolOutput) -> JsonRpcResponse {
    let text =
        serde_json::to_string(&output).expect("MCP contract output serialization must not fail");
    let structured_content =
        serde_json::to_value(output).expect("MCP contract output serialization must not fail");
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured_content
        }),
    )
}

fn mcp_tool_error(id: serde_json::Value, error: NoteUseCaseError) -> JsonRpcResponse {
    let value = match error {
        NoteUseCaseError::Validation(diagnostics) => validation_problem_json(diagnostics),
        NoteUseCaseError::NotFound => {
            serde_json::json!({"code":"not_found","message":"note was not found"})
        }
        NoteUseCaseError::Conflict => {
            serde_json::json!({"code":"conflict","message":"note revision conflicts"})
        }
        NoteUseCaseError::RenderFailed => serde_json::json!({
            "code":"render_failed",
            "message":"note cannot be rendered safely"
        }),
        NoteUseCaseError::Unavailable => serde_json::json!({
            "code":"unavailable",
            "message":"note service is unavailable"
        }),
    };
    let text = serde_json::to_string(&value).unwrap_or_else(|_| {
        r#"{"code":"unavailable","message":"note service is unavailable"}"#.into()
    });
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "content":[{"type":"text","text":text}],
            "structuredContent":value,
            "isError":true
        }),
    )
}

fn note_profile_output(profile: NoteProfile) -> McpNoteProfileOutput {
    McpNoteProfileOutput {
        profile_version: profile.profile_version,
        adocweave_package_version: profile.adocweave_package_version.into(),
        limits: McpNoteProfileLimits {
            applies_after_normalization: true,
            max_title_characters: profile.limits.max_title_characters,
            max_source_bytes: profile.limits.max_source_bytes,
            max_tags: profile.limits.max_tags,
            max_tag_characters: profile.limits.max_tag_characters,
        },
        normalization: McpNoteProfileNormalization {
            title: profile
                .normalization
                .title
                .into_iter()
                .map(str::to_owned)
                .collect(),
            tags: profile
                .normalization
                .tags
                .into_iter()
                .map(str::to_owned)
                .collect(),
        },
        syntax: McpNoteProfileSyntax {
            common_blocks: profile
                .syntax
                .common_blocks
                .into_iter()
                .map(str::to_owned)
                .collect(),
            common_inlines: profile
                .syntax
                .common_inlines
                .into_iter()
                .map(str::to_owned)
                .collect(),
            source_language_optional: profile.syntax.source_language_optional,
            allowed_math_languages: profile
                .syntax
                .allowed_math_languages
                .into_iter()
                .map(str::to_owned)
                .collect(),
            title_forbidden: profile
                .syntax
                .title_forbidden
                .into_iter()
                .map(str::to_owned)
                .collect(),
            tag_forbidden: profile
                .syntax
                .tag_forbidden
                .into_iter()
                .map(str::to_owned)
                .collect(),
        },
        authoring_guidance: profile
            .authoring_guidance
            .into_iter()
            .map(str::to_owned)
            .collect(),
        allowed_source_languages: profile
            .allowed_source_languages
            .into_iter()
            .map(str::to_owned)
            .collect(),
        forbidden_rules: profile
            .forbidden_rules
            .into_iter()
            .map(|rule| McpNoteProfileRule {
                code: rule.code.as_str().into(),
                description: rule.description.into(),
            })
            .collect(),
        examples: profile
            .examples
            .into_iter()
            .map(|example| McpNoteProfileExample {
                kind: example.kind.into(),
                description: example.description.into(),
                body: example.body.into(),
            })
            .collect(),
    }
}

fn note_revision_output(note: Note) -> McpToolOutput {
    McpToolOutput::Revision(McpNoteRevisionOutput {
        note_id: note.note_id().to_string(),
        revision: note.revision().get(),
    })
}
