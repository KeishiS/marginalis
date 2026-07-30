//! 利用者ごとの書誌ライブラリーREST API。

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use marginalis_application::{BibliographyUseCaseError, BibliographyUseCases};
use marginalis_contract::{
    BibliographyItemInput, BibliographyItemResponse, ProblemCode, ProblemResponse,
};
use marginalis_domain::{BibliographyItem, BibliographyItemId, EntityId};
use serde::Deserialize;

use super::{
    auth::{authenticated_actor, authenticated_mutation_actor},
    error::{HandlerResult, problem},
    notes::{etag, expected_revision},
    state::ApiState,
};

#[derive(Default, Deserialize)]
pub(super) struct BibliographyQuery {
    #[serde(default)]
    query: String,
}

pub(super) async fn search_bibliography(
    State(state): State<ApiState>,
    Query(query): Query<BibliographyQuery>,
    headers: HeaderMap,
) -> HandlerResult<Json<Vec<BibliographyItemResponse>>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let bibliography = bibliography(&state)?;
    let items = bibliography
        .search_bibliography(actor, query.query)
        .await
        .map_err(bibliography_error)?;
    Ok(Json(items.into_iter().map(item_response).collect()))
}

pub(super) async fn add_bibliography_item(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<BibliographyItemInput>,
) -> HandlerResult<Response> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let item = bibliography(&state)?
        .add_bibliography_item(actor, input.csl_json)
        .await
        .map_err(bibliography_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::ETAG, etag(item.revision()))],
        Json(item_response(item)),
    )
        .into_response())
}

pub(super) async fn delete_bibliography_item(
    State(state): State<ApiState>,
    Path(item_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<StatusCode> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let item_id = parse_item_id(item_id)?;
    bibliography(&state)?
        .delete_bibliography_item(actor, item_id, expected_revision(&headers)?)
        .await
        .map_err(bibliography_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn update_bibliography_item(
    State(state): State<ApiState>,
    Path(item_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<BibliographyItemInput>,
) -> HandlerResult<Response> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let item_id = parse_item_id(item_id)?;
    let item = bibliography(&state)?
        .update_bibliography_item(actor, item_id, expected_revision(&headers)?, input.csl_json)
        .await
        .map_err(bibliography_error)?;
    Ok((
        StatusCode::OK,
        [(header::ETAG, etag(item.revision()))],
        Json(item_response(item)),
    )
        .into_response())
}

fn parse_item_id(item_id: String) -> HandlerResult<BibliographyItemId> {
    item_id
        .parse::<EntityId>()
        .map(BibliographyItemId::new)
        .map_err(|_| {
            problem(
                StatusCode::BAD_REQUEST,
                ProblemCode::InvalidRequest,
                "item_id is invalid",
            )
        })
}

fn bibliography(state: &ApiState) -> HandlerResult<&dyn BibliographyUseCases> {
    state.bibliography.as_deref().ok_or_else(|| {
        problem(
            StatusCode::SERVICE_UNAVAILABLE,
            ProblemCode::Unavailable,
            "bibliography service is unavailable",
        )
    })
}

fn bibliography_error(error: BibliographyUseCaseError) -> (StatusCode, Json<ProblemResponse>) {
    match error {
        BibliographyUseCaseError::InvalidCslJson => problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            ProblemCode::ValidationFailed,
            "CSL-JSON must contain valid id and type fields",
        ),
        BibliographyUseCaseError::NotFound => problem(
            StatusCode::NOT_FOUND,
            ProblemCode::NotFound,
            "bibliography item was not found",
        ),
        BibliographyUseCaseError::Conflict => problem(
            StatusCode::CONFLICT,
            ProblemCode::Conflict,
            "citation key already exists or revision conflicts",
        ),
        BibliographyUseCaseError::Unavailable => problem(
            StatusCode::SERVICE_UNAVAILABLE,
            ProblemCode::Unavailable,
            "bibliography service is unavailable",
        ),
    }
}

fn item_response(item: BibliographyItem) -> BibliographyItemResponse {
    BibliographyItemResponse {
        item_id: item.item_id().to_string(),
        citation_key: item.citation_key().to_owned(),
        csl_json: serde_json::from_str(item.csl_json()).expect("stored CSL-JSON is valid"),
        created_at_ms: item.created_at().get(),
        updated_at_ms: item.updated_at().get(),
        revision: item.revision().get(),
    }
}
