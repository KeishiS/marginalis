//! ノート所有者ごとのMathJaxマクロ設定API。

use axum::{Json, extract::State, http::HeaderMap};
use marginalis_application::MathMacro;
use marginalis_contract::{MathMacroResponse, MathMacroSettingsInput, MathMacroSettingsResponse};

use super::{
    auth::{authenticated_actor, authenticated_mutation_actor},
    error::{HandlerResult, math_macro_error},
    state::ApiState,
};

pub(super) async fn read_math_macros(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Json<MathMacroSettingsResponse>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let settings = state
        .math_macros
        .read_math_macros(actor)
        .await
        .map_err(math_macro_error)?;
    Ok(Json(settings_response(settings)))
}

pub(super) async fn replace_math_macros(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<MathMacroSettingsInput>,
) -> HandlerResult<Json<MathMacroSettingsResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let settings = state
        .math_macros
        .replace_math_macros(
            actor,
            input
                .macros
                .into_iter()
                .map(|item| MathMacro {
                    name: item.name,
                    replacement: item.replacement,
                    argument_count: item.argument_count,
                })
                .collect(),
            input.revision,
        )
        .await
        .map_err(math_macro_error)?;
    Ok(Json(settings_response(settings)))
}

fn settings_response(
    settings: marginalis_application::MathMacroSettings,
) -> MathMacroSettingsResponse {
    MathMacroSettingsResponse {
        macros: settings
            .macros
            .into_iter()
            .map(|item| MathMacroResponse {
                name: item.name,
                replacement: item.replacement,
                argument_count: item.argument_count,
            })
            .collect(),
        revision: settings.revision,
    }
}
