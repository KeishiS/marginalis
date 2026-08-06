//! 現在の利用者がMCPクライアントへ許可できるscope上限の設定API。

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use marginalis_contract::{
    McpClientAuthorizationResponse, McpScopeCeilingInput, McpScopeCeilingResponse,
};
use serde::Deserialize;

/// 解除する上限のrevisionをquery parameterで受け取る。
#[derive(Deserialize)]
pub(super) struct McpScopeCeilingRevisionQuery {
    revision: i64,
}

use super::{
    auth::{authenticated_actor, authenticated_mutation_actor},
    error::{HandlerResult, mcp_scope_ceiling_error},
    mcp_endpoint,
    state::ApiState,
};

pub(super) async fn read_mcp_scope_ceiling(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Json<McpScopeCeilingResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let endpoint = mcp_endpoint(&state)?;
    let setting = endpoint
        .oauth
        .principal_scope_ceiling(actor)
        .await
        .map_err(mcp_scope_ceiling_error)?;
    Ok(Json(scope_ceiling_response(
        endpoint.resource_policy.supported_scopes(),
        setting,
    )))
}

pub(super) async fn replace_mcp_scope_ceiling(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<McpScopeCeilingInput>,
) -> HandlerResult<Json<McpScopeCeilingResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let endpoint = mcp_endpoint(&state)?;
    let setting = endpoint
        .oauth
        .replace_principal_scope_ceiling(actor, input.scopes, input.revision)
        .await
        .map_err(mcp_scope_ceiling_error)?;
    Ok(Json(scope_ceiling_response(
        endpoint.resource_policy.supported_scopes(),
        setting,
    )))
}

pub(super) async fn list_mcp_authorizations(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Json<Vec<McpClientAuthorizationResponse>>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let endpoint = mcp_endpoint(&state)?;
    let authorizations = endpoint
        .oauth
        .client_authorizations(actor)
        .await
        .map_err(mcp_scope_ceiling_error)?;
    Ok(Json(
        authorizations
            .into_iter()
            .map(client_authorization_response)
            .collect(),
    ))
}

pub(super) async fn replace_client_mcp_scope_ceiling(
    State(state): State<ApiState>,
    Path(client_id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<McpScopeCeilingInput>,
) -> HandlerResult<Json<McpClientAuthorizationResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let endpoint = mcp_endpoint(&state)?;
    let setting = endpoint
        .oauth
        .replace_client_scope_ceiling(
            actor.clone(),
            client_id.clone(),
            input.scopes,
            input.revision,
        )
        .await
        .map_err(mcp_scope_ceiling_error)?;
    let mut authorization = endpoint
        .oauth
        .client_authorizations(actor)
        .await
        .map_err(mcp_scope_ceiling_error)?
        .into_iter()
        .find(|authorization| authorization.client_id == client_id)
        .ok_or_else(|| {
            mcp_scope_ceiling_error(
                marginalis_application::McpScopeCeilingUseCaseError::ClientNotFound,
            )
        })?;
    authorization.scope_ceiling.configured = true;
    authorization.scope_ceiling.setting = setting;
    Ok(Json(client_authorization_response(authorization)))
}

/// clientの上限設定を取り除き、未設定の状態を返す。
///
/// 上限は将来の認可だけを制限するため、解除しても既存のtokenへ権限は増えない。
pub(super) async fn delete_client_mcp_scope_ceiling(
    State(state): State<ApiState>,
    Path(client_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<McpScopeCeilingRevisionQuery>,
) -> HandlerResult<Json<McpClientAuthorizationResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let endpoint = mcp_endpoint(&state)?;
    endpoint
        .oauth
        .delete_client_scope_ceiling(actor.clone(), client_id.clone(), query.revision)
        .await
        .map_err(mcp_scope_ceiling_error)?;
    let authorization = endpoint
        .oauth
        .client_authorizations(actor)
        .await
        .map_err(mcp_scope_ceiling_error)?
        .into_iter()
        .find(|authorization| authorization.client_id == client_id)
        .ok_or_else(|| {
            mcp_scope_ceiling_error(
                marginalis_application::McpScopeCeilingUseCaseError::ClientNotFound,
            )
        })?;
    Ok(Json(client_authorization_response(authorization)))
}

fn client_authorization_response(
    authorization: marginalis_application::McpClientAuthorization,
) -> McpClientAuthorizationResponse {
    let registration_method = match authorization.registration_method {
        marginalis_application::McpClientRegistrationMethod::Dynamic => "dynamic",
        marginalis_application::McpClientRegistrationMethod::MetadataDocument => {
            "metadata_document"
        }
    };
    McpClientAuthorizationResponse {
        client_id: authorization.client_id,
        display_name: authorization.display_name,
        registration_method: registration_method.into(),
        granted_scopes: authorization.granted_scopes,
        scope_ceiling_configured: authorization.scope_ceiling.configured,
        scope_ceiling: authorization.scope_ceiling.setting.scopes,
        scope_ceiling_revision: authorization.scope_ceiling.setting.revision,
        authorized_at_ms: authorization.authorized_at.get(),
        last_used_at_ms: authorization.last_used_at.map(|timestamp| timestamp.get()),
        active: authorization.active,
    }
}

fn scope_ceiling_response(
    supported_scopes: &[String],
    setting: marginalis_application::McpScopeCeilingSetting,
) -> McpScopeCeilingResponse {
    McpScopeCeilingResponse {
        supported_scopes: supported_scopes.to_vec(),
        scopes: setting.scopes,
        revision: setting.revision,
    }
}
