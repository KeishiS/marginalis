//! MCP toolの入力検査、use case呼出し、契約型への変換。

use marginalis_application::{
    BibliographyApplication, BibliographyUseCaseError, NoteListQuery, NoteProfile,
    NoteUseCaseError, NoteUseCases, NoteWritePolicy,
};
use marginalis_contract::{
    McpAddBibliographyItemInput, McpAddBibliographyItemsInput, McpBibliographyImportError,
    McpBibliographyImportOutput, McpBibliographyItem, McpBibliographyListOutput,
    McpCreateNoteInput, McpDeleteBibliographyItemInput, McpDeleteNoteInput, McpEmptyInput,
    McpGetNoteInput, McpGetNoteOutput, McpListNotesInput, McpListNotesOutput,
    McpNoteProfileExample, McpNoteProfileLimits, McpNoteProfileNormalization, McpNoteProfileOutput,
    McpNoteProfileRule, McpNoteProfileSyntax, McpNoteRevisionOutput, McpSearchBibliographyInput,
    McpToolName, McpUpdateNoteInput, ProblemResponse,
};
use marginalis_domain::{
    Actor, BibliographyItemId, EntityId, Note, NoteCreationSource, NoteDraft, Revision,
};
use serde::{Deserialize, Serialize};

use crate::mcp::JsonRpcResponse;

use super::super::{
    auth::parse_note_id,
    error::{bibliography_problem, note_problem},
};

pub(super) struct McpToolCall {
    tool: Option<McpToolName>,
    arguments: serde_json::Value,
}

impl McpToolCall {
    pub(super) fn scope_requirements(&self) -> &'static [&'static [&'static str]] {
        self.tool.map_or(&[], McpToolName::scope_requirements)
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
    BibliographyList(McpBibliographyListOutput),
    BibliographyItem(McpBibliographyItem),
    BibliographyImport(McpBibliographyImportOutput),
    Empty(McpEmptyInput),
}

pub(super) async fn mcp_tool_call(
    notes: &dyn NoteUseCases,
    bibliography: Option<&BibliographyApplication>,
    actor: Actor,
    id: serde_json::Value,
    call: McpToolCall,
) -> JsonRpcResponse {
    let tool = call.tool.map_or("unknown", McpToolName::as_str);
    match execute_mcp_tool(notes, bibliography, actor, call).await {
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
                McpToolFailure::Bibliography(error) => mcp_bibliography_error(id, error),
            }
        }
    }
}

enum McpToolFailure {
    InvalidArguments(&'static str),
    UnknownTool,
    UseCase(NoteUseCaseError),
    Bibliography(BibliographyUseCaseError),
}

impl McpToolFailure {
    fn outcome(&self) -> &'static str {
        match self {
            Self::UseCase(NoteUseCaseError::Unavailable | NoteUseCaseError::CorruptData) => {
                "failure"
            }
            Self::Bibliography(
                BibliographyUseCaseError::Unavailable | BibliographyUseCaseError::CorruptData,
            ) => "failure",
            Self::InvalidArguments(_)
            | Self::UnknownTool
            | Self::UseCase(
                NoteUseCaseError::Validation(_)
                | NoteUseCaseError::AdvisoriesRejected(_)
                | NoteUseCaseError::NotFound
                | NoteUseCaseError::Conflict
                | NoteUseCaseError::RetentionExpired
                | NoteUseCaseError::RenderFailed,
            )
            | Self::Bibliography(_) => "rejected",
        }
    }

    fn reason(&self) -> &'static str {
        match self {
            Self::InvalidArguments(_) => "invalid-arguments",
            Self::UnknownTool => "unknown-tool",
            Self::UseCase(NoteUseCaseError::Validation(_)) => "validation",
            Self::UseCase(NoteUseCaseError::AdvisoriesRejected(_)) => "warning",
            Self::UseCase(NoteUseCaseError::NotFound) => "not-found",
            Self::UseCase(NoteUseCaseError::Conflict) => "conflict",
            Self::UseCase(NoteUseCaseError::RetentionExpired) => "retention-expired",
            Self::UseCase(NoteUseCaseError::RenderFailed) => "render-failed",
            Self::UseCase(NoteUseCaseError::Unavailable) => "unavailable",
            Self::UseCase(NoteUseCaseError::CorruptData) => "corrupt-data",
            Self::Bibliography(BibliographyUseCaseError::CorruptData) => "corrupt-data",
            Self::Bibliography(BibliographyUseCaseError::InvalidSearchQuery) => {
                "invalid-search-query"
            }
            Self::Bibliography(BibliographyUseCaseError::InvalidCslJson) => "invalid-csl-json",
            Self::Bibliography(BibliographyUseCaseError::NotFound) => "not-found",
            Self::Bibliography(BibliographyUseCaseError::Conflict) => "conflict",
            Self::Bibliography(BibliographyUseCaseError::Unavailable) => "unavailable",
        }
    }
}

async fn execute_mcp_tool(
    notes: &dyn NoteUseCases,
    bibliography: Option<&BibliographyApplication>,
    actor: Actor,
    call: McpToolCall,
) -> Result<McpToolOutput, McpToolFailure> {
    let result = match call.tool {
        Some(McpToolName::ListNotes) => {
            let Ok(input) = serde_json::from_value::<McpListNotesInput>(call.arguments) else {
                return Err(McpToolFailure::InvalidArguments(
                    "list arguments are invalid",
                ));
            };
            notes
                .list_visible_notes(
                    actor,
                    NoteListQuery {
                        created_via: input.created_via,
                        review_status: input.review_status,
                    },
                )
                .await
                .map(|notes| {
                    McpToolOutput::NoteList(McpListNotesOutput {
                        notes: notes
                            .into_iter()
                            .map(|entry| crate::http::notes::note_summary_response(entry.summary))
                            .collect(),
                    })
                })
        }
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
                    created_via: note.created_via(),
                    review_status: note.review_status(),
                    reviewed_revision: note.last_review().map(|review| review.revision().get()),
                    reviewed_at_ms: note.last_review().map(|review| review.reviewed_at().get()),
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
                    NoteWritePolicy::RejectWarnings,
                    NoteCreationSource::Mcp,
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
                    NoteWritePolicy::RejectWarnings,
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
        Some(McpToolName::SearchBibliography) => {
            let Ok(input) = serde_json::from_value::<McpSearchBibliographyInput>(call.arguments)
            else {
                return Err(McpToolFailure::InvalidArguments(
                    "bibliography search arguments are invalid",
                ));
            };
            let Some(bibliography) = bibliography else {
                return Err(McpToolFailure::Bibliography(
                    BibliographyUseCaseError::Unavailable,
                ));
            };
            return bibliography
                .search_bibliography(actor, input.query)
                .await
                .map(|items| {
                    McpToolOutput::BibliographyList(McpBibliographyListOutput {
                        items: items.into_iter().map(bibliography_item_output).collect(),
                    })
                })
                .map_err(McpToolFailure::Bibliography);
        }
        Some(McpToolName::AddBibliographyItem) => {
            let Ok(input) = serde_json::from_value::<McpAddBibliographyItemInput>(call.arguments)
            else {
                return Err(McpToolFailure::InvalidArguments(
                    "CSL-JSON bibliography arguments are invalid",
                ));
            };
            let Some(bibliography) = bibliography else {
                return Err(McpToolFailure::Bibliography(
                    BibliographyUseCaseError::Unavailable,
                ));
            };
            return bibliography
                .add_bibliography_item(actor, input.csl_json)
                .await
                .map(|item| McpToolOutput::BibliographyItem(bibliography_item_output(item)))
                .map_err(McpToolFailure::Bibliography);
        }
        Some(McpToolName::AddBibliographyItems) => {
            let Ok(input) = serde_json::from_value::<McpAddBibliographyItemsInput>(call.arguments)
            else {
                return Err(McpToolFailure::InvalidArguments(
                    "CSL-JSON bibliography batch arguments are invalid",
                ));
            };
            if input.csl_json_items.is_empty() || input.csl_json_items.len() > 100 {
                return Err(McpToolFailure::InvalidArguments(
                    "csl_json_items must contain between 1 and 100 items",
                ));
            }
            let Some(bibliography) = bibliography else {
                return Err(McpToolFailure::Bibliography(
                    BibliographyUseCaseError::Unavailable,
                ));
            };
            let mut items = Vec::new();
            let mut errors = Vec::new();
            for (input_index, csl_json) in input.csl_json_items.into_iter().enumerate() {
                let citation_key = csl_json
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                match bibliography
                    .add_bibliography_item(actor.clone(), csl_json)
                    .await
                {
                    Ok(item) => items.push(bibliography_item_output(item)),
                    Err(error) => {
                        errors.push(bibliography_import_error(input_index, citation_key, error))
                    }
                }
            }
            return Ok(McpToolOutput::BibliographyImport(
                McpBibliographyImportOutput { items, errors },
            ));
        }
        Some(McpToolName::DeleteBibliographyItem) => {
            let Ok(input) =
                serde_json::from_value::<McpDeleteBibliographyItemInput>(call.arguments)
            else {
                return Err(McpToolFailure::InvalidArguments(
                    "bibliography delete arguments are invalid",
                ));
            };
            let Ok(entity_id) = input.item_id.parse::<EntityId>() else {
                return Err(McpToolFailure::InvalidArguments("item_id is invalid"));
            };
            let Ok(expected_revision) = Revision::new(input.expected_revision) else {
                return Err(McpToolFailure::InvalidArguments(
                    "expected_revision is invalid",
                ));
            };
            let Some(bibliography) = bibliography else {
                return Err(McpToolFailure::Bibliography(
                    BibliographyUseCaseError::Unavailable,
                ));
            };
            return bibliography
                .delete_bibliography_item(
                    actor,
                    BibliographyItemId::new(entity_id),
                    expected_revision,
                )
                .await
                .map(|()| McpToolOutput::Empty(McpEmptyInput {}))
                .map_err(McpToolFailure::Bibliography);
        }
        None => return Err(McpToolFailure::UnknownTool),
    };
    result.map_err(McpToolFailure::UseCase)
}

fn bibliography_import_error(
    input_index: usize,
    citation_key: Option<String>,
    error: BibliographyUseCaseError,
) -> McpBibliographyImportError {
    // 項目別の失敗も、単独の失敗と同じ写像から`code`と`message`を得る。
    let problem = bibliography_problem(error);
    McpBibliographyImportError {
        input_index,
        citation_key,
        code: problem.code.as_str().into(),
        message: problem.message,
    }
}

fn bibliography_item_output(item: marginalis_domain::BibliographyItem) -> McpBibliographyItem {
    McpBibliographyItem {
        item_id: item.item_id().to_string(),
        citation_key: item.citation_key().to_owned(),
        csl_json: serde_json::from_str(item.csl_json())
            .expect("stored CSL-JSON was validated before persistence"),
        updated_at_ms: item.updated_at().get(),
        revision: item.revision().get(),
    }
}

fn mcp_bibliography_error(
    id: serde_json::Value,
    error: BibliographyUseCaseError,
) -> JsonRpcResponse {
    mcp_problem_result(id, bibliography_problem(error))
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
    mcp_problem_result(id, note_problem(error))
}

/// 失敗をRESTと同じ`ProblemResponse`として返す。
///
/// 手でJSONを組み立てず、`error`moduleの写像だけを使う。`docs/mcp-tools.json`が示す
/// 失敗出力schemaと同じ型を通すため、契約検査が実行時応答との差を検出できる。
fn mcp_problem_result(id: serde_json::Value, problem: ProblemResponse) -> JsonRpcResponse {
    let value = serde_json::to_value(&problem).expect("problem response is serializable");
    let text = serde_json::to_string(&problem).expect("problem response is serializable");
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
            allowed_document_attributes: profile
                .syntax
                .allowed_document_attributes
                .into_iter()
                .map(str::to_owned)
                .collect(),
            allowed_citation_styles: profile
                .syntax
                .allowed_citation_styles
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
