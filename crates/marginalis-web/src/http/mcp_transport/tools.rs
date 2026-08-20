//! MCP toolの入力検査、use case呼出し、契約型への変換。

use marginalis_application::{
    BibliographyApplication, BibliographyUseCaseError, NoteAdvisorySeverity, NoteListQuery,
    NoteProfile, NoteUseCaseError, NoteUseCases, NoteWritePolicy,
};
use marginalis_contract::{
    DiagnosticSeverityResponse, McpAddBibliographyItemInput, McpApplyNotePatchInput,
    McpBibliographyItem, McpBibliographyListOutput, McpCreateNoteInput,
    McpDeleteBibliographyItemInput, McpDeleteNoteInput, McpEmptyInput, McpGetNoteFragmentInput,
    McpGetNoteInput, McpGetNoteOutlineInput, McpGetNoteOutput, McpListNoteTemplatesOutput,
    McpListNotesInput, McpListNotesOutput, McpNoteFragmentOutput, McpNoteOutlineOutput,
    McpNoteOutlineSection, McpNotePatchOutput, McpNoteProfileAdvisoryRule, McpNoteProfileExample,
    McpNoteProfileLimits, McpNoteProfileNormalization, McpNoteProfileOutput, McpNoteProfileRule,
    McpNoteProfileSyntax, McpNoteRevisionOutput, McpReplaceNoteSourceInput,
    McpSearchBibliographyInput, McpToolName, ProblemResponse,
};
use marginalis_domain::{
    Actor, BibliographyItemId, EntityId, Note, NoteCreationSource, NoteDraft, Revision,
};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::jsonrpc::JsonRpcResponse;

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
    NoteTemplateList(McpListNoteTemplatesOutput),
    NoteProfile(Box<McpNoteProfileOutput>),
    Note(McpGetNoteOutput),
    NoteOutline(McpNoteOutlineOutput),
    NoteFragment(McpNoteFragmentOutput),
    NotePatch(McpNotePatchOutput),
    Revision(McpNoteRevisionOutput),
    BibliographyList(McpBibliographyListOutput),
    BibliographyItem(McpBibliographyItem),
    Empty(McpEmptyInput),
}

impl McpToolOutput {
    fn text(&self) -> String {
        match self {
            Self::NoteList(output) => {
                let count = output.notes.len();
                let mut text = format!(
                    "{count} visible {}.",
                    if count == 1 { "note" } else { "notes" }
                );
                for note in &output.notes {
                    let _ = write!(
                        text,
                        "\n- {} — {} (revision {})",
                        note.note_id, note.title, note.revision
                    );
                }
                text
            }
            Self::NoteTemplateList(output) => {
                let count = output.templates.len();
                let mut text = format!(
                    "{count} template {}.",
                    if count == 1 { "note" } else { "notes" }
                );
                for note in &output.templates {
                    let _ = write!(
                        text,
                        "\n- {} — {} (revision {})",
                        note.note_id, note.title, note.revision
                    );
                }
                text
            }
            Self::NoteProfile(output) => {
                let mut text = format!(
                    "Note profile version {} (AdocWeave {}, Marginalis {}).\nMCP note writes reject warnings: {}.\nLimits after normalization: title {} characters, source {} bytes, {} tags, {} characters per tag.",
                    output.profile_version,
                    output.adocweave_package_version,
                    output.marginalis_version,
                    output.warnings_reject_write,
                    output.limits.max_title_characters,
                    output.limits.max_source_bytes,
                    output.limits.max_tags,
                    output.limits.max_tag_characters,
                );
                text.push_str("\nTitle normalization:");
                for rule in &output.normalization.title {
                    let _ = write!(text, "\n- {rule}");
                }
                text.push_str("\nTag normalization:");
                for rule in &output.normalization.tags {
                    let _ = write!(text, "\n- {rule}");
                }
                let _ = write!(
                    text,
                    "\nCommon blocks: {}.\nCommon inline forms: {}.\nSource block language is optional: {}.",
                    output.syntax.common_blocks.join(", "),
                    output.syntax.common_inlines.join(", "),
                    output.syntax.source_language_optional,
                );
                let _ = write!(
                    text,
                    "\nAllowed source languages: {}.\nAllowed math languages: {}.\nAllowed document attributes: {}.\nAllowed citation styles: {}.",
                    output.allowed_source_languages.join(", "),
                    output.syntax.allowed_math_languages.join(", "),
                    output.syntax.allowed_document_attributes.join(", "),
                    output.syntax.allowed_citation_styles.join(", "),
                );
                let _ = write!(
                    text,
                    "\nForbidden title values: {}.\nForbidden tag values: {}.",
                    output.syntax.title_forbidden.join(", "),
                    output.syntax.tag_forbidden.join(", "),
                );
                text.push_str("\nAdvisory rules:");
                for rule in &output.advisory_rules {
                    let _ = write!(
                        text,
                        "\n- {} ({}): {}",
                        rule.code,
                        diagnostic_severity_name(rule.severity),
                        rule.description
                    );
                }
                text.push_str("\nForbidden rules:");
                for rule in &output.forbidden_rules {
                    let _ = write!(text, "\n- {}: {}", rule.code, rule.description);
                }
                text.push_str("\nAuthoring guidance:");
                for guidance in &output.authoring_guidance {
                    let _ = write!(text, "\n- {guidance}");
                }
                text.push_str("\nExamples:");
                for example in &output.examples {
                    let _ = write!(
                        text,
                        "\n- {} — {}:\n{}",
                        example.kind,
                        example.description,
                        indent(&example.body)
                    );
                }
                text
            }
            Self::Note(note) => note_text(note),
            Self::NoteOutline(outline) => {
                let mut text = format!(
                    "Note {} — {} (revision {}, {} lines).",
                    outline.note_id, outline.title, outline.revision, outline.line_count
                );
                if outline.sections.is_empty() {
                    text.push_str("\nNo section headings.");
                } else {
                    text.push_str("\nSections (level, lines, anchor):");
                    for section in &outline.sections {
                        let _ = write!(
                            text,
                            "\n- {} {} (level {}, lines {}-{}{})",
                            "=".repeat(usize::from(section.level) + 1),
                            section.title,
                            section.level,
                            section.start_line,
                            section.end_line,
                            section
                                .anchor
                                .as_deref()
                                .map(|anchor| format!(", anchor #{anchor}"))
                                .unwrap_or_default(),
                        );
                    }
                }
                text
            }
            Self::NoteFragment(fragment) => format!(
                "Note {} (revision {}), lines {}-{}:\n{}",
                fragment.note_id,
                fragment.revision,
                fragment.start_line,
                fragment.end_line,
                indent(&fragment.fragment)
            ),
            Self::NotePatch(patch) => {
                let mut text = if patch.dry_run {
                    format!(
                        "Dry run: patch for note {} would apply {} hunk(s) (+{} -{} lines) and passes validation; nothing was saved.",
                        patch.note_id, patch.hunks_applied, patch.lines_added, patch.lines_removed
                    )
                } else {
                    format!(
                        "Applied {} hunk(s) (+{} -{} lines) to note {}; now at revision {}.",
                        patch.hunks_applied,
                        patch.lines_added,
                        patch.lines_removed,
                        patch.note_id,
                        patch.revision.unwrap_or_default()
                    )
                };
                if !patch.diagnostics.is_empty() {
                    text.push_str("\nDiagnostics:");
                    for diagnostic in &patch.diagnostics {
                        let _ = write!(
                            text,
                            "\n- {} ({}): {}",
                            diagnostic.code,
                            diagnostic_severity_name(diagnostic.severity),
                            diagnostic.message
                        );
                    }
                }
                text
            }
            Self::Revision(output) => format!(
                "Note {} is at revision {}.",
                output.note_id, output.revision
            ),
            Self::BibliographyList(output) => {
                let count = output.items.len();
                let mut text = format!(
                    "{count} bibliography {}.",
                    if count == 1 { "item" } else { "items" }
                );
                for item in &output.items {
                    let _ = write!(
                        text,
                        "\n- {} — {} (revision {})",
                        item.item_id, item.citation_key, item.revision
                    );
                }
                text
            }
            Self::BibliographyItem(item) => {
                let csl = serde_json::to_string_pretty(&item.csl_json)
                    .expect("CSL-JSON contract output serialization must not fail");
                format!(
                    "Bibliography item {} — {} (revision {}).\nCSL-JSON:\n{}",
                    item.item_id,
                    item.citation_key,
                    item.revision,
                    indent(&csl)
                )
            }
            Self::Empty(_) => "Operation completed.".into(),
        }
    }
}

fn note_text(note: &McpGetNoteOutput) -> String {
    format!(
        "Note {} — {} (revision {}).\nTags: {}\nAsciiDoc source:\n{}",
        note.note_id,
        note.title,
        note.revision,
        if note.tags.is_empty() {
            "(none)".into()
        } else {
            note.tags.join(", ")
        },
        indent(&note.source)
    )
}

fn indent(value: &str) -> String {
    value
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

const fn diagnostic_severity_name(severity: DiagnosticSeverityResponse) -> &'static str {
    match severity {
        DiagnosticSeverityResponse::Error => "error",
        DiagnosticSeverityResponse::Warning => "warning",
        DiagnosticSeverityResponse::Information => "information",
        DiagnosticSeverityResponse::Hint => "hint",
    }
}

pub(super) async fn mcp_tool_call(
    notes: &dyn NoteUseCases,
    bibliography: &BibliographyApplication,
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
                | NoteUseCaseError::InvalidSyncLimit
                | NoteUseCaseError::InvalidSyncCursor
                | NoteUseCaseError::SyncCursorExpired
                | NoteUseCaseError::InvalidLineRange
                | NoteUseCaseError::PatchRejected(_)
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
            Self::UseCase(NoteUseCaseError::InvalidSyncLimit) => "invalid-sync-limit",
            Self::UseCase(NoteUseCaseError::InvalidSyncCursor) => "invalid-sync-cursor",
            Self::UseCase(NoteUseCaseError::SyncCursorExpired) => "sync-cursor-expired",
            Self::UseCase(NoteUseCaseError::InvalidLineRange) => "invalid-line-range",
            Self::UseCase(NoteUseCaseError::PatchRejected(_)) => "patch-rejected",
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

/// tool名でツールごとの実装関数へ振り分ける。入力検査と呼出しは各関数が持つ。
async fn execute_mcp_tool(
    notes: &dyn NoteUseCases,
    bibliography: &BibliographyApplication,
    actor: Actor,
    call: McpToolCall,
) -> Result<McpToolOutput, McpToolFailure> {
    match call.tool {
        Some(McpToolName::ListNotes) => list_notes_tool(notes, actor, call.arguments).await,
        Some(McpToolName::ListNoteTemplates) => {
            list_note_templates_tool(notes, actor, &call.arguments).await
        }
        Some(McpToolName::GetNoteProfile) => get_note_profile_tool(notes, &call.arguments),
        Some(McpToolName::GetNote) => get_note_tool(notes, actor, call.arguments).await,
        Some(McpToolName::GetNoteOutline) => {
            get_note_outline_tool(notes, actor, call.arguments).await
        }
        Some(McpToolName::GetNoteFragment) => {
            get_note_fragment_tool(notes, actor, call.arguments).await
        }
        Some(McpToolName::CreateNote) => create_note_tool(notes, actor, call.arguments).await,
        Some(McpToolName::ApplyNotePatch) => {
            apply_note_patch_tool(notes, actor, call.arguments).await
        }
        Some(McpToolName::ReplaceNoteSource) => {
            replace_note_source_tool(notes, actor, call.arguments).await
        }
        Some(McpToolName::DeleteNote) => delete_note_tool(notes, actor, call.arguments).await,
        Some(McpToolName::SearchBibliography) => {
            search_bibliography_tool(bibliography, actor, call.arguments).await
        }
        Some(McpToolName::AddBibliographyItem) => {
            add_bibliography_item_tool(bibliography, actor, call.arguments).await
        }
        Some(McpToolName::DeleteBibliographyItem) => {
            delete_bibliography_item_tool(bibliography, actor, call.arguments).await
        }
        None => Err(McpToolFailure::UnknownTool),
    }
}

async fn list_notes_tool(
    notes: &dyn NoteUseCases,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpListNotesInput>(arguments) else {
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
        .map_err(McpToolFailure::UseCase)
}

async fn list_note_templates_tool(
    notes: &dyn NoteUseCases,
    actor: Actor,
    arguments: &serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    if arguments.as_object().is_none_or(|value| !value.is_empty()) {
        return Err(McpToolFailure::InvalidArguments(
            "template list arguments are invalid",
        ));
    }
    notes
        .list_note_templates(actor)
        .await
        .map(|templates| {
            McpToolOutput::NoteTemplateList(McpListNoteTemplatesOutput {
                templates: templates
                    .into_iter()
                    .map(|entry| crate::http::notes::note_summary_response(entry.summary))
                    .collect(),
            })
        })
        .map_err(McpToolFailure::UseCase)
}

fn get_note_profile_tool(
    notes: &dyn NoteUseCases,
    arguments: &serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    if arguments.as_object().is_none_or(|value| !value.is_empty()) {
        return Err(McpToolFailure::InvalidArguments(
            "profile arguments are invalid",
        ));
    }
    Ok(McpToolOutput::NoteProfile(Box::new(note_profile_output(
        notes.note_profile(),
    ))))
}

async fn get_note_tool(
    notes: &dyn NoteUseCases,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpGetNoteInput>(arguments) else {
        return Err(McpToolFailure::InvalidArguments(
            "get arguments are invalid",
        ));
    };
    let Some(note_id) = parse_note_id(&input.note_id).ok() else {
        return Err(McpToolFailure::InvalidArguments("note_id is invalid"));
    };
    notes
        .read_note(actor, note_id)
        .await
        .map(|note| McpToolOutput::Note(note_output(note)))
        .map_err(McpToolFailure::UseCase)
}

fn note_output(note: Note) -> McpGetNoteOutput {
    McpGetNoteOutput {
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
    }
}

async fn create_note_tool(
    notes: &dyn NoteUseCases,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpCreateNoteInput>(arguments) else {
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
        .map_err(McpToolFailure::UseCase)
}

async fn get_note_outline_tool(
    notes: &dyn NoteUseCases,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpGetNoteOutlineInput>(arguments) else {
        return Err(McpToolFailure::InvalidArguments(
            "outline arguments are invalid",
        ));
    };
    let Some(note_id) = parse_note_id(&input.note_id).ok() else {
        return Err(McpToolFailure::InvalidArguments("note_id is invalid"));
    };
    notes
        .read_note_outline(actor, note_id)
        .await
        .map(|(note, outline)| {
            McpToolOutput::NoteOutline(McpNoteOutlineOutput {
                note_id: note.note_id().to_string(),
                title: note.title().to_owned(),
                revision: note.revision().get(),
                line_count: outline.line_count,
                sections: outline
                    .sections
                    .into_iter()
                    .map(|section| McpNoteOutlineSection {
                        level: section.level,
                        title: section.title,
                        anchor: section.anchor,
                        start_line: section.start_line,
                        end_line: section.end_line,
                    })
                    .collect(),
            })
        })
        .map_err(McpToolFailure::UseCase)
}

async fn get_note_fragment_tool(
    notes: &dyn NoteUseCases,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpGetNoteFragmentInput>(arguments) else {
        return Err(McpToolFailure::InvalidArguments(
            "fragment arguments are invalid",
        ));
    };
    let Some(note_id) = parse_note_id(&input.note_id).ok() else {
        return Err(McpToolFailure::InvalidArguments("note_id is invalid"));
    };
    let expected_revision = match input.expected_revision {
        None => None,
        Some(revision) => match Revision::new(revision) {
            Ok(revision) => Some(revision),
            Err(_) => {
                return Err(McpToolFailure::InvalidArguments(
                    "expected_revision is invalid",
                ));
            }
        },
    };
    notes
        .read_note_fragment(
            actor,
            note_id,
            input.start_line,
            input.end_line,
            expected_revision,
        )
        .await
        .map(|(note, fragment)| {
            McpToolOutput::NoteFragment(McpNoteFragmentOutput {
                note_id: note.note_id().to_string(),
                revision: note.revision().get(),
                start_line: input.start_line,
                end_line: input.end_line,
                fragment,
            })
        })
        .map_err(McpToolFailure::UseCase)
}

async fn apply_note_patch_tool(
    notes: &dyn NoteUseCases,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpApplyNotePatchInput>(arguments) else {
        return Err(McpToolFailure::InvalidArguments(
            "patch arguments are invalid",
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
        .apply_note_patch(
            actor,
            note_id,
            &input.patch,
            expected_revision,
            NoteWritePolicy::RejectWarnings,
            input.dry_run,
        )
        .await
        .map(|applied| {
            McpToolOutput::NotePatch(McpNotePatchOutput {
                note_id: input.note_id,
                revision: applied.note.map(|note| note.revision().get()),
                dry_run: input.dry_run,
                hunks_applied: applied.hunks_applied,
                lines_added: applied.lines_added,
                lines_removed: applied.lines_removed,
                diagnostics: applied
                    .advisories
                    .into_iter()
                    .map(super::super::error::advisory_response)
                    .collect(),
            })
        })
        .map_err(McpToolFailure::UseCase)
}

async fn replace_note_source_tool(
    notes: &dyn NoteUseCases,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpReplaceNoteSourceInput>(arguments) else {
        return Err(McpToolFailure::InvalidArguments(
            "replace arguments are invalid",
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
        .map_err(McpToolFailure::UseCase)
}

async fn delete_note_tool(
    notes: &dyn NoteUseCases,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpDeleteNoteInput>(arguments) else {
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
        .map_err(McpToolFailure::UseCase)
}

async fn search_bibliography_tool(
    bibliography: &BibliographyApplication,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpSearchBibliographyInput>(arguments) else {
        return Err(McpToolFailure::InvalidArguments(
            "bibliography search arguments are invalid",
        ));
    };
    bibliography
        .search_bibliography(actor, input.query)
        .await
        .map(|items| {
            McpToolOutput::BibliographyList(McpBibliographyListOutput {
                items: items.into_iter().map(bibliography_item_output).collect(),
            })
        })
        .map_err(McpToolFailure::Bibliography)
}

async fn add_bibliography_item_tool(
    bibliography: &BibliographyApplication,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpAddBibliographyItemInput>(arguments) else {
        return Err(McpToolFailure::InvalidArguments(
            "CSL-JSON bibliography arguments are invalid",
        ));
    };
    bibliography
        .add_bibliography_item(actor, input.csl_json)
        .await
        .map(|item| McpToolOutput::BibliographyItem(bibliography_item_output(item)))
        .map_err(McpToolFailure::Bibliography)
}

async fn delete_bibliography_item_tool(
    bibliography: &BibliographyApplication,
    actor: Actor,
    arguments: serde_json::Value,
) -> Result<McpToolOutput, McpToolFailure> {
    let Ok(input) = serde_json::from_value::<McpDeleteBibliographyItemInput>(arguments) else {
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
    bibliography
        .delete_bibliography_item(actor, BibliographyItemId::new(entity_id), expected_revision)
        .await
        .map(|()| McpToolOutput::Empty(McpEmptyInput {}))
        .map_err(McpToolFailure::Bibliography)
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
    let text = output.text();
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
    let text = problem_text(&problem);
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "content":[{"type":"text","text":text}],
            "structuredContent":value,
            "isError":true
        }),
    )
}

fn problem_text(problem: &ProblemResponse) -> String {
    let mut text = format!("{} ({}).", problem.message, problem.code.as_str());
    for diagnostic in &problem.diagnostics {
        let location = diagnostic.position.map_or_else(String::new, |position| {
            format!(" at line {}, column {}", position.line, position.column)
        });
        let _ = write!(
            text,
            "\n- {} {}{location}: {}",
            diagnostic_severity_name(diagnostic.severity),
            diagnostic.code,
            diagnostic.message
        );
    }
    text
}

fn note_profile_output(profile: NoteProfile) -> McpNoteProfileOutput {
    McpNoteProfileOutput {
        profile_version: profile.profile_version,
        adocweave_package_version: profile.adocweave_package_version.into(),
        // workspace全体で版を共有するため、このcrateの版がserverの版と一致する。
        marginalis_version: env!("CARGO_PKG_VERSION").to_owned(),
        limits: McpNoteProfileLimits {
            applies_after_normalization: true,
            max_title_characters: profile.limits.max_title_characters,
            max_source_bytes: profile.limits.max_source_bytes,
            max_patch_bytes: profile.limits.max_patch_bytes,
            max_patch_hunks: profile.limits.max_patch_hunks,
            max_tags: profile.limits.max_tags,
            max_tag_characters: profile.limits.max_tag_characters,
            max_attachment_bytes: profile.limits.max_attachment_bytes,
            max_attachments_per_note: profile.limits.max_attachments_per_note,
            max_attachment_bytes_per_note: profile.limits.max_attachment_bytes_per_note,
            max_attachment_file_name_characters: profile.limits.max_attachment_file_name_characters,
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
        advisory_rules: profile
            .advisory_rules
            .into_iter()
            .map(|rule| McpNoteProfileAdvisoryRule {
                code: rule.code.into(),
                description: rule.description.into(),
                severity: match rule.severity {
                    NoteAdvisorySeverity::Warning => DiagnosticSeverityResponse::Warning,
                    NoteAdvisorySeverity::Information => DiagnosticSeverityResponse::Information,
                    NoteAdvisorySeverity::Hint => DiagnosticSeverityResponse::Hint,
                },
            })
            .collect(),
        warnings_reject_write: true,
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
