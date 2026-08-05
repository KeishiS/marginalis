//! 現在の利用者がMCPクライアントへ許可できるscope上限の設定API。

use axum::{Json, extract::State, http::HeaderMap};
use marginalis_contract::{McpScopeCeilingInput, McpScopeCeilingResponse};

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
