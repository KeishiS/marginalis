//! CSL-JSON一方向取り込みのREST adapter。

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use marginalis_application::{
    BibliographyImportClassification, BibliographyImportDecision, BibliographyImportDecisionKind,
    BibliographyImportInput, BibliographyImportSourceSelection, BibliographyImportUseCases,
};
use marginalis_contract::{
    BibliographyImportApplyInput, BibliographyImportCandidateResponse,
    BibliographyImportClassificationResponse, BibliographyImportDecisionKindInput,
    BibliographyImportEntryResponse, BibliographyImportPreviewInput,
    BibliographyImportPreviewResponse, BibliographyImportResultResponse,
    BibliographyImportSourceInput, BibliographyImportSourceResponse, ProblemCode,
};
use marginalis_domain::{
    BibliographyImportMethod, BibliographyImportSource, BibliographyImportSourceId, EntityId,
    Revision,
};

use super::{
    auth::{authenticated_actor, authenticated_mutation_actor},
    bibliography::parse_item_id,
    error::{HandlerResult, bibliography_import_error, problem},
    state::ApiState,
};

pub(super) async fn list_bibliography_import_sources(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Json<Vec<BibliographyImportSourceResponse>>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let sources = bibliography_import(&state)?
        .list_bibliography_import_sources(actor)
        .await
        .map_err(bibliography_import_error)?;
    Ok(Json(sources.into_iter().map(source_response).collect()))
}

pub(super) async fn preview_bibliography_import(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<BibliographyImportPreviewInput>,
) -> HandlerResult<Json<BibliographyImportPreviewResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let input = import_input(input.source, input.items)?;
    let preview = bibliography_import(&state)?
        .preview_bibliography_import(actor, input)
        .await
        .map_err(bibliography_import_error)?;
    Ok(Json(BibliographyImportPreviewResponse {
        source_id: preview.source_id.map(|source_id| source_id.to_string()),
        source_revision: preview.source_revision.map(Revision::get),
        preview_token: preview.preview_token,
        entries: preview
            .entries
            .into_iter()
            .map(|entry| BibliographyImportEntryResponse {
                position: entry.position,
                external_item_id: entry.external_item_id,
                citation_key: entry.citation_key,
                classification: classification_response(entry.classification),
                item_id: entry.item_id.map(|item_id| item_id.to_string()),
                item_revision: entry.item_revision.map(Revision::get),
                current_csl_json: entry.current_csl_json,
                candidates: entry
                    .candidates
                    .into_iter()
                    .map(|candidate| BibliographyImportCandidateResponse {
                        item_id: candidate.item_id.to_string(),
                        citation_key: candidate.citation_key,
                        title: candidate.title,
                        revision: candidate.revision.get(),
                        matched_by: candidate.matched_by,
                    })
                    .collect(),
                rejection_code: entry.rejection_code,
            })
            .collect(),
    }))
}

pub(super) async fn apply_bibliography_import(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<BibliographyImportApplyInput>,
) -> HandlerResult<Json<BibliographyImportResultResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let import_input = import_input(input.source, input.items)?;
    let decisions = input
        .decisions
        .into_iter()
        .map(|decision| {
            Ok(BibliographyImportDecision {
                position: decision.position,
                kind: decision_kind(decision.action),
                candidate_item_id: decision.candidate_item_id.map(parse_item_id).transpose()?,
            })
        })
        .collect::<HandlerResult<Vec<_>>>()?;
    let result = bibliography_import(&state)?
        .apply_bibliography_import(actor, import_input, decisions, input.preview_token)
        .await
        .map_err(bibliography_import_error)?;
    Ok(Json(BibliographyImportResultResponse {
        source_id: result.source_id.to_string(),
        source_revision: result.source_revision.get(),
        created: result.created,
        updated: result.updated,
        kept: result.kept,
        excluded: result.excluded,
    }))
}

fn classification_response(
    classification: BibliographyImportClassification,
) -> BibliographyImportClassificationResponse {
    match classification {
        BibliographyImportClassification::Create => {
            BibliographyImportClassificationResponse::Create
        }
        BibliographyImportClassification::UpdateFromExternal => {
            BibliographyImportClassificationResponse::UpdateFromExternal
        }
        BibliographyImportClassification::Unchanged => {
            BibliographyImportClassificationResponse::Unchanged
        }
        BibliographyImportClassification::KeepLocal => {
            BibliographyImportClassificationResponse::KeepLocal
        }
        BibliographyImportClassification::Conflict => {
            BibliographyImportClassificationResponse::Conflict
        }
        BibliographyImportClassification::DuplicateCandidate => {
            BibliographyImportClassificationResponse::DuplicateCandidate
        }
        BibliographyImportClassification::Rejected => {
            BibliographyImportClassificationResponse::Rejected
        }
    }
}

fn decision_kind(input: BibliographyImportDecisionKindInput) -> BibliographyImportDecisionKind {
    match input {
        BibliographyImportDecisionKindInput::ApplySuggested => {
            BibliographyImportDecisionKind::ApplySuggested
        }
        BibliographyImportDecisionKindInput::CreateSeparate => {
            BibliographyImportDecisionKind::CreateSeparate
        }
        BibliographyImportDecisionKindInput::KeepLocal => BibliographyImportDecisionKind::KeepLocal,
        BibliographyImportDecisionKindInput::UseExternal => {
            BibliographyImportDecisionKind::UseExternal
        }
        BibliographyImportDecisionKindInput::LinkExistingKeepLocal => {
            BibliographyImportDecisionKind::LinkExistingKeepLocal
        }
        BibliographyImportDecisionKindInput::LinkExistingUseExternal => {
            BibliographyImportDecisionKind::LinkExistingUseExternal
        }
        BibliographyImportDecisionKindInput::Exclude => BibliographyImportDecisionKind::Exclude,
    }
}

fn import_input(
    source: BibliographyImportSourceInput,
    items: Vec<serde_json::Value>,
) -> HandlerResult<BibliographyImportInput> {
    Ok(BibliographyImportInput {
        source: match source {
            BibliographyImportSourceInput::New { display_name } => {
                BibliographyImportSourceSelection::New { display_name }
            }
            BibliographyImportSourceInput::Existing { source_id } => {
                BibliographyImportSourceSelection::Existing {
                    source_id: parse_source_id(source_id)?,
                }
            }
        },
        items,
    })
}

fn parse_source_id(source_id: String) -> HandlerResult<BibliographyImportSourceId> {
    source_id
        .parse::<EntityId>()
        .map(BibliographyImportSourceId::new)
        .map_err(|_| {
            problem(
                StatusCode::BAD_REQUEST,
                ProblemCode::InvalidRequest,
                "source_id is invalid",
            )
        })
}

fn bibliography_import(state: &ApiState) -> HandlerResult<&dyn BibliographyImportUseCases> {
    state.bibliography_import.as_deref().ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            ProblemCode::Unavailable,
            "bibliography import service is unavailable",
        )
    })
}

fn source_response(source: BibliographyImportSource) -> BibliographyImportSourceResponse {
    BibliographyImportSourceResponse {
        source_id: source.source_id().to_string(),
        method: match source.method() {
            BibliographyImportMethod::CslJsonFile => "csl_json_file".into(),
        },
        display_name: source.display_name().to_owned(),
        revision: source.revision().get(),
        created_at_ms: source.created_at().get(),
        last_imported_at_ms: source.last_imported_at().get(),
    }
}
