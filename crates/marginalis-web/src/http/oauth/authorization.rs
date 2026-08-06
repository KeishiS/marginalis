//! Marginalisが提供するMCP OAuth authorization server境界。

use axum::{
    Json,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use marginalis_application::{
    McpAuthorizationRequest, McpOAuthUseCaseError, McpValidatedAuthorizationRequest,
};
use marginalis_contract::ProblemCode;
use marginalis_domain::Actor;
use sha2::Sha256;

use super::super::{
    auth::{
        CSRF_COOKIE, SESSION_COOKIE, authenticated_actor, authenticated_form_actor, cookie_value,
        external_path,
    },
    error::{HandlerResult, problem},
    html::{escape_html, page_document},
    mcp_endpoint,
    state::{ApiState, McpEndpoint},
};
use super::common::{OAuthParameters, content_type_is, log_mcp_oauth_result, oauth_error_response};

const AUTHORIZATION_PARAMETERS: &[&str] = &[
    "response_type",
    "client_id",
    "redirect_uri",
    "resource",
    "scope",
    "code_challenge",
    "code_challenge_method",
    "state",
];
const MAX_LOGIN_RESUME_PATH_BYTES: usize = 2_800;

pub(crate) async fn mcp_resource_metadata(
    State(state): State<ApiState>,
) -> HandlerResult<Json<mcp_authorization_server::ProtectedResourceMetadata>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(endpoint.resource_policy.protected_resource_metadata(
        endpoint.authorization_server_uri.clone(),
    )))
}

pub(crate) async fn mcp_server_metadata(
    State(state): State<ApiState>,
) -> HandlerResult<Json<mcp_authorization_server::AuthorizationServerMetadata>> {
    let endpoint = mcp_endpoint(&state)?;
    Ok(Json(
        endpoint
            .resource_policy
            .authorization_server_metadata(&endpoint.authorization_server_endpoints),
    ))
}

#[derive(Clone)]
struct McpAuthorizeInput {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    resource: Option<String>,
    scope: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    state: Option<String>,
}

impl McpAuthorizeInput {
    fn from_parameters(parameters: &OAuthParameters) -> Self {
        Self {
            response_type: parameters.get("response_type").map(str::to_owned),
            client_id: parameters.get("client_id").map(str::to_owned),
            redirect_uri: parameters.get("redirect_uri").map(str::to_owned),
            resource: parameters.get("resource").map(str::to_owned),
            scope: parameters.get("scope").map(str::to_owned),
            code_challenge: parameters.get("code_challenge").map(str::to_owned),
            code_challenge_method: parameters.get("code_challenge_method").map(str::to_owned),
            state: parameters.get("state").map(str::to_owned),
        }
    }

    fn scopes(&self) -> Vec<String> {
        self.scope
            .as_deref()
            .unwrap_or_default()
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect()
    }
}

pub(crate) struct McpAuthorizeForm {
    client_id: String,
    redirect_uri: Option<String>,
    resource: String,
    scope: String,
    code_challenge: String,
    state: Option<String>,
    csrf_token: String,
    consent_signature: String,
    selected_scopes: Vec<String>,
    decision: String,
}

impl McpAuthorizeForm {
    fn parse(body: &[u8]) -> Option<Self> {
        let mut values = std::collections::HashMap::new();
        let mut selected_scopes = Vec::new();
        for (name, value) in url::form_urlencoded::parse(body) {
            if name == "selected_scope" {
                if value.is_empty() {
                    return None;
                }
                selected_scopes.push(value.into_owned());
                continue;
            }
            if !matches!(
                name.as_ref(),
                "client_id"
                    | "redirect_uri"
                    | "resource"
                    | "scope"
                    | "code_challenge"
                    | "state"
                    | "csrf_token"
                    | "consent_signature"
                    | "decision"
            ) || values
                .insert(name.into_owned(), value.into_owned())
                .is_some()
            {
                return None;
            }
        }
        Some(Self {
            client_id: values.remove("client_id")?,
            redirect_uri: values.remove("redirect_uri"),
            resource: values.remove("resource")?,
            scope: values.remove("scope")?,
            code_challenge: values.remove("code_challenge")?,
            state: values.remove("state"),
            csrf_token: values.remove("csrf_token")?,
            consent_signature: values.remove("consent_signature")?,
            selected_scopes,
            decision: values.remove("decision")?,
        })
    }
}

pub(crate) async fn mcp_authorize(
    State(state): State<ApiState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Result<Response, Response> {
    let mut parameters = OAuthParameters::default();
    parameters.append(
        raw_query.as_deref().unwrap_or_default(),
        AUTHORIZATION_PARAMETERS,
    );
    let result = mcp_authorize_request(&state, &headers, parameters).await;
    log_mcp_oauth_result("authorization", &result);
    result
}

/// OAuth clientからの初回POSTはqueryとform bodyの両方を受け付けるが、状態を変更しない。
pub(crate) async fn mcp_authorize_post(
    State(state): State<ApiState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    body: Bytes,
) -> Result<Response, Response> {
    let mut parameters = OAuthParameters::default();
    parameters.append(
        raw_query.as_deref().unwrap_or_default(),
        AUTHORIZATION_PARAMETERS,
    );
    if !body.is_empty() {
        if !content_type_is(&headers, "application/x-www-form-urlencoded") {
            return Err(oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "authorization request body must be form encoded",
            ));
        }
        let encoded = std::str::from_utf8(&body).map_err(|_| {
            oauth_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "authorization request encoding is invalid",
            )
        })?;
        parameters.append(encoded, AUTHORIZATION_PARAMETERS);
    }
    let result = mcp_authorize_request(&state, &headers, parameters).await;
    log_mcp_oauth_result("authorization", &result);
    result
}

async fn mcp_authorize_request(
    state: &ApiState,
    headers: &HeaderMap,
    parameters: OAuthParameters,
) -> Result<Response, Response> {
    let endpoint = mcp_endpoint(state).map_err(|error| error.into_response())?;
    let input = McpAuthorizeInput::from_parameters(&parameters);
    let client_id = input.client_id.clone().ok_or_else(|| {
        oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "client_id is required",
        )
    })?;
    let resolved = endpoint
        .oauth
        .resolve_authorization_client(client_id.clone(), input.redirect_uri.clone())
        .await
        .map_err(unsafe_authorization_error)?;
    if parameters.repeated {
        return Ok(oauth_redirect_error(
            &endpoint.authorization_server_uri,
            &resolved.redirect_uri,
            input.state.as_deref(),
            "invalid_request",
        ));
    }
    if input.response_type.as_deref() != Some("code") {
        return Ok(oauth_redirect_error(
            &endpoint.authorization_server_uri,
            &resolved.redirect_uri,
            input.state.as_deref(),
            "unsupported_response_type",
        ));
    }
    if input.code_challenge_method.as_deref() != Some("S256") {
        return Ok(oauth_redirect_error(
            &endpoint.authorization_server_uri,
            &resolved.redirect_uri,
            input.state.as_deref(),
            "invalid_request",
        ));
    }
    let request = McpAuthorizationRequest {
        client_id,
        redirect_uri: input.redirect_uri.clone(),
        resource_uri: input.resource.clone().unwrap_or_default(),
        scopes: input.scopes(),
        code_challenge: input.code_challenge.clone().unwrap_or_default(),
    };
    let error_redirect_uri = resolved.redirect_uri.clone();
    let mut validated = endpoint
        .oauth
        .validate_resolved_authorization_request(request, resolved)
        .await
        .map_err(|error| {
            safe_authorization_error(
                &endpoint.authorization_server_uri,
                &error_redirect_uri,
                input.state.as_deref(),
                error,
            )
        })?;
    let actor = match authenticated_actor(headers, state).await {
        Ok(actor) => actor,
        Err((StatusCode::UNAUTHORIZED, _)) => {
            return Ok(
                login_redirect(state, &input, &validated).unwrap_or_else(|| {
                    oauth_redirect_error(
                        &endpoint.authorization_server_uri,
                        validated.redirect_uri.as_str(),
                        input.state.as_deref(),
                        "invalid_request",
                    )
                }),
            );
        }
        Err(error) => return Err(error.into_response()),
    };
    let withheld = apply_scope_ceilings(endpoint, actor, &mut validated).await?;
    // 上限が要求scopeをすべて除いた場合は、選べる権限がない同意画面を出さずclientへ返す。
    if validated.scopes.is_empty() && !withheld.is_empty() {
        return Ok(oauth_redirect_error(
            &endpoint.authorization_server_uri,
            validated.redirect_uri.as_str(),
            input.state.as_deref(),
            "invalid_scope",
        ));
    }
    let csrf = cookie_value(headers, CSRF_COOKIE).ok_or_else(|| {
        problem(
            StatusCode::FORBIDDEN,
            ProblemCode::CsrfRequired,
            "CSRF token is required",
        )
        .into_response()
    })?;
    let session_id = cookie_value(headers, SESSION_COOKIE).expect("authenticated session exists");
    Ok(consent_page(
        state,
        &input,
        &validated,
        &withheld,
        &csrf,
        &session_id,
        None,
    ))
}

/// 要求scopeからscope上限で許可できないものを取り除き、取り除いた分を返す。
///
/// 同意画面と認可で同じ上限を使い、表示した権限だけが付与されるようにする。
async fn apply_scope_ceilings(
    endpoint: &McpEndpoint,
    actor: Actor,
    validated: &mut McpValidatedAuthorizationRequest,
) -> Result<Vec<String>, Response> {
    let grantable = endpoint
        .oauth
        .grantable_scopes(
            actor,
            validated.client.client_id.clone(),
            validated.scopes.clone(),
        )
        .await
        .map_err(unsafe_authorization_error)?;
    let withheld = validated
        .scopes
        .iter()
        .filter(|scope| !grantable.contains(scope))
        .cloned()
        .collect::<Vec<_>>();
    if !withheld.is_empty() {
        tracing::info!(
            event = "mcp.oauth.scope.withheld",
            operation = "authorization",
            withheld_scopes = withheld.join(" "),
            "requested MCP scopes exceed the configured ceiling"
        );
    }
    validated.scopes = grantable;
    Ok(withheld)
}

fn login_redirect(
    state: &ApiState,
    input: &McpAuthorizeInput,
    request: &McpValidatedAuthorizationRequest,
) -> Option<Response> {
    let mut request_uri = url::Url::parse("https://invalid.example/oauth/authorize")
        .expect("constant authorization URL is valid");
    {
        let mut pairs = request_uri.query_pairs_mut();
        pairs.append_pair("response_type", "code");
        pairs.append_pair("client_id", &request.client.client_id);
        if request.redirect_uri.was_supplied() {
            pairs.append_pair("redirect_uri", request.redirect_uri.as_str());
        }
        pairs.append_pair("resource", &request.resource_uri);
        pairs.append_pair("scope", &request.scopes.join(" "));
        pairs.append_pair("code_challenge", &request.code_challenge);
        pairs.append_pair("code_challenge_method", "S256");
        if let Some(state) = &input.state {
            pairs.append_pair("state", state);
        }
    }
    let next = format!(
        "{}?{}",
        external_path(&state.cookie_path, "/oauth/authorize"),
        request_uri.query().expect("query pairs were added")
    );
    if next.len() > MAX_LOGIN_RESUME_PATH_BYTES {
        return None;
    }
    let encoded_next = url::form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>();
    Some(
        Redirect::to(&format!(
            "{}?next={encoded_next}",
            external_path(&state.cookie_path, "/auth/oidc/login")
        ))
        .into_response(),
    )
}

fn consent_page(
    state: &ApiState,
    input: &McpAuthorizeInput,
    request: &McpValidatedAuthorizationRequest,
    withheld_scopes: &[String],
    csrf: &str,
    signature_key: &str,
    selection_error: Option<&str>,
) -> Response {
    let consent_path = external_path(&state.cookie_path, "/oauth/authorize/consent");
    let redirect = url::Url::parse(request.redirect_uri.as_str()).ok();
    let redirect_host = redirect
        .as_ref()
        .and_then(url::Url::host_str)
        .unwrap_or("確認できません");
    let content = format!(
        concat!(
            "<section class=\"oauth-consent-page page-section\" aria-labelledby=\"oauth-consent-heading\">",
            "<div class=\"page-heading\"><div>",
            "<p class=\"page-eyebrow\">MCP access</p>",
            "<h1 id=\"oauth-consent-heading\">MCPクライアントを許可しますか？</h1>",
            "<p class=\"page-description\">許可する前に、接続するクライアントと要求された権限を確認してください。</p>",
            "</div></div>",
            "<div class=\"oauth-consent surface\">",
            "{client_summary}{scope_section}{withheld_section}{loopback_warning}{consent_form}",
            "</div></section>",
        ),
        client_summary = consent_client_summary(request, redirect_host),
        scope_section = consent_scope_section(&request.scopes, selection_error),
        withheld_section = consent_withheld_section(withheld_scopes),
        loopback_warning = consent_loopback_warning(redirect.as_ref()),
        consent_form = consent_form(&consent_path, input, request, csrf, signature_key),
    );
    Html(page_document(
        "MCPクライアントの認可",
        &state.cookie_path,
        &content,
        &[],
    ))
    .into_response()
}

fn consent_client_summary(
    request: &McpValidatedAuthorizationRequest,
    redirect_host: &str,
) -> String {
    format!(
        concat!(
            "<section class=\"oauth-client\" aria-labelledby=\"oauth-client-heading\">",
            "<p class=\"oauth-detail-label\">クライアント識別子</p>",
            "<h2 id=\"oauth-client-heading\" class=\"oauth-client-id\"><code>{client_id}</code></h2>",
            "<dl class=\"oauth-detail-list\">",
            "<div><dt>クライアントが提供した表示名</dt><dd>{display_name}</dd></div>",
            "<div><dt>移動先のホスト</dt><dd><code>{redirect_host}</code></dd></div>",
            "</dl></section>",
        ),
        display_name = escape_html(&request.client.display_name),
        client_id = escape_html(&request.client.client_id),
        redirect_host = escape_html(redirect_host),
    )
}

fn consent_scope_section(scopes: &[String], selection_error: Option<&str>) -> String {
    let error = selection_error
        .map(|message| {
            format!(
                "<p class=\"problem-message\" role=\"alert\">{}</p>",
                escape_html(message)
            )
        })
        .unwrap_or_default();
    let content = if scopes.is_empty() {
        "<p class=\"state-message\">要求された権限はありません。</p>".into()
    } else {
        let items = scopes
            .iter()
            .map(|scope| {
                format!(
                    concat!(
                        "<li><label>",
                        "<input type=\"checkbox\" name=\"selected_scope\" value=\"{scope}\" ",
                        "form=\"oauth-consent-form\" checked>",
                        "<span><code>{scope}</code><span>{description}</span></span>",
                        "</label></li>",
                    ),
                    scope = escape_html(scope),
                    description = scope_description(scope),
                )
            })
            .collect::<String>();
        format!("<ul class=\"oauth-scope-list\">{items}</ul>")
    };
    format!(
        concat!(
            "<section class=\"oauth-scope-section\" aria-labelledby=\"oauth-scope-heading\">",
            "<h2 id=\"oauth-scope-heading\">許可する権限</h2>",
            "<p>このクライアントに許可する操作を選択してください。</p>",
            "{error}{content}</section>",
        ),
        error = error,
        content = content,
    )
}

/// 上限で許可できない要求scopeを、許可できる権限と区別して知らせる。
///
/// 表示しないだけでは、クライアントが要求した権限がなぜ有効にならないのか分からない。
fn consent_withheld_section(scopes: &[String]) -> String {
    if scopes.is_empty() {
        return String::new();
    }
    let items = scopes
        .iter()
        .map(|scope| {
            format!(
                "<li><code>{scope}</code><span>{description}</span></li>",
                scope = escape_html(scope),
                description = scope_description(scope),
            )
        })
        .collect::<String>();
    format!(
        concat!(
            "<aside class=\"oauth-withheld-scopes warnings\" aria-labelledby=\"oauth-withheld-heading\">",
            "<h2 id=\"oauth-withheld-heading\">許可できない権限があります</h2>",
            "<p>このクライアントは次の権限も要求しましたが、あなたが設定したscope上限を超えるため、",
            "ここでは許可できません。必要な場合は、MCPアクセス設定で上限を広げてから、",
            "クライアント側でもう一度認可してください。</p>",
            "<ul class=\"oauth-scope-list\">{items}</ul></aside>",
        ),
        items = items,
    )
}

fn scope_description(scope: &str) -> &'static str {
    match scope {
        "notes:read" => "list_notes、get_note、get_note_profileでノートを読み取ります。",
        "notes:write" => "create_note、update_note、get_note_profileでノートを作成・更新します。",
        "notes:delete" => "delete_noteでノートを削除します。",
        "bibliography:read" => "search_bibliographyで書誌情報を検索します。",
        "bibliography:write" => {
            "add_bibliography_item、add_bibliography_itemsで書誌情報を追加します。"
        }
        "bibliography:delete" => "delete_bibliography_itemで書誌情報を削除します。",
        _ => "このクライアントが要求した権限です。",
    }
}

fn consent_loopback_warning(redirect: Option<&url::Url>) -> &'static str {
    if redirect.is_some_and(is_loopback_redirect) {
        concat!(
            "<aside class=\"oauth-loopback-warning warnings\">",
            "<h2>この端末上のアプリへ戻ります</h2>",
            "<p>許可すると、処理を続けるため、この端末で動作しているアプリへ移動します。</p>",
            "</aside>",
        )
    } else {
        ""
    }
}

fn consent_form(
    consent_path: &str,
    input: &McpAuthorizeInput,
    request: &McpValidatedAuthorizationRequest,
    csrf: &str,
    signature_key: &str,
) -> String {
    let mut fields = hidden_input("client_id", &request.client.client_id);
    if request.redirect_uri.was_supplied() {
        fields.push_str(&hidden_input("redirect_uri", request.redirect_uri.as_str()));
    }
    fields.push_str(&hidden_input("resource", &request.resource_uri));
    fields.push_str(&hidden_input("scope", &request.scopes.join(" ")));
    fields.push_str(&hidden_input("code_challenge", &request.code_challenge));
    if let Some(state) = &input.state {
        fields.push_str(&hidden_input("state", state));
    }
    fields.push_str(&hidden_input("csrf_token", csrf));
    fields.push_str(&hidden_input(
        "consent_signature",
        &consent_signature(signature_key, input, request),
    ));
    format!(
        concat!(
            "<form id=\"oauth-consent-form\" class=\"oauth-consent-actions\" method=\"post\" action=\"{action}\">",
            "{fields}",
            "<button class=\"button button-primary\" name=\"decision\" value=\"approve\">許可する</button>",
            "<button class=\"button button-secondary\" name=\"decision\" value=\"deny\">拒否する</button>",
            "</form>",
        ),
        action = escape_html(consent_path),
        fields = fields,
    )
}

/// HttpOnlyのsession IDを鍵にして、同意画面へ表示した要求を同じsessionへ結び付ける。
fn consent_signature(
    signature_key: &str,
    input: &McpAuthorizeInput,
    request: &McpValidatedAuthorizationRequest,
) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(signature_key.as_bytes())
        .expect("HMAC accepts keys of every size");
    update_signature(&mut mac, &request.client.client_id);
    update_signature(
        &mut mac,
        if request.redirect_uri.was_supplied() {
            "1"
        } else {
            "0"
        },
    );
    update_signature(&mut mac, request.redirect_uri.as_str());
    update_signature(&mut mac, &request.resource_uri);
    update_signature(&mut mac, &request.scopes.join(" "));
    update_signature(&mut mac, &request.code_challenge);
    update_signature(&mut mac, input.state.as_deref().unwrap_or_default());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn verify_consent_signature(
    signature_key: &str,
    input: &McpAuthorizeInput,
    request: &McpValidatedAuthorizationRequest,
    signature: &str,
) -> bool {
    let Ok(signature) = URL_SAFE_NO_PAD.decode(signature) else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(signature_key.as_bytes())
        .expect("HMAC accepts keys of every size");
    update_signature(&mut mac, &request.client.client_id);
    update_signature(
        &mut mac,
        if request.redirect_uri.was_supplied() {
            "1"
        } else {
            "0"
        },
    );
    update_signature(&mut mac, request.redirect_uri.as_str());
    update_signature(&mut mac, &request.resource_uri);
    update_signature(&mut mac, &request.scopes.join(" "));
    update_signature(&mut mac, &request.code_challenge);
    update_signature(&mut mac, input.state.as_deref().unwrap_or_default());
    mac.verify_slice(&signature).is_ok()
}

fn update_signature(mac: &mut Hmac<Sha256>, value: &str) {
    mac.update(&value.len().to_be_bytes());
    mac.update(value.as_bytes());
}

fn hidden_input(name: &str, value: &str) -> String {
    format!(
        "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
        escape_html(name),
        escape_html(value),
    )
}

fn is_loopback_redirect(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

/// Marginalis自身が表示した承認form専用の状態変更endpoint。
pub(crate) async fn mcp_authorize_consent(
    State(state): State<ApiState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Response> {
    if !content_type_is(&headers, "application/x-www-form-urlencoded") {
        return Err(oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "authorization consent must be form encoded",
        ));
    }
    let form = McpAuthorizeForm::parse(&body).ok_or_else(|| {
        oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "authorization consent form is invalid",
        )
    })?;
    let result = mcp_authorize_consent_inner(state, headers, form).await;
    log_mcp_oauth_result("consent", &result);
    result
}

async fn mcp_authorize_consent_inner(
    state: ApiState,
    headers: HeaderMap,
    form: McpAuthorizeForm,
) -> Result<Response, Response> {
    let endpoint = mcp_endpoint(&state).map_err(|error| error.into_response())?;
    let actor = authenticated_form_actor(&headers, &state, &form.csrf_token)
        .await
        .map_err(|error| error.into_response())?;
    let session_id = cookie_value(&headers, SESSION_COOKIE).expect("authenticated session exists");
    let state_value = form.state.as_deref().filter(|value| !value.is_empty());
    let input = McpAuthorizeInput {
        response_type: Some("code".into()),
        client_id: Some(form.client_id.clone()),
        redirect_uri: form.redirect_uri.clone(),
        resource: Some(form.resource.clone()),
        scope: Some(form.scope.clone()),
        code_challenge: Some(form.code_challenge.clone()),
        code_challenge_method: Some("S256".into()),
        state: form.state.clone(),
    };
    let request = McpAuthorizationRequest {
        client_id: form.client_id,
        redirect_uri: form.redirect_uri,
        resource_uri: form.resource,
        scopes: form
            .scope
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect(),
        code_challenge: form.code_challenge,
    };
    let mut validated = endpoint
        .oauth
        .validate_authorization_request(request)
        .await
        .map_err(unsafe_authorization_error)?;
    if !verify_consent_signature(&session_id, &input, &validated, &form.consent_signature) {
        return Err(oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "authorization consent form was modified",
        ));
    }
    match form.decision.as_str() {
        "deny" => Ok(oauth_redirect_error(
            &endpoint.authorization_server_uri,
            validated.redirect_uri.as_str(),
            state_value,
            "access_denied",
        )),
        "approve" => {
            // 同意画面を表示してから上限が狭まった場合に、選んだ権限を黙って削らない。
            let withheld = apply_scope_ceilings(endpoint, actor.clone(), &mut validated).await?;
            if !withheld.is_empty() {
                return Ok((
                    StatusCode::CONFLICT,
                    consent_page(
                        &state,
                        &input,
                        &validated,
                        &withheld,
                        &form.csrf_token,
                        &session_id,
                        Some("scope上限が変更されたため、選択できる権限が変わりました。内容を確認してもう一度選択してください。"),
                    ),
                )
                    .into_response());
            }
            if form.selected_scopes.is_empty() {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    consent_page(
                        &state,
                        &input,
                        &validated,
                        &withheld,
                        &form.csrf_token,
                        &session_id,
                        Some("少なくとも1つの権限を選択するか、拒否してください。"),
                    ),
                )
                    .into_response());
            }
            if form.selected_scopes.iter().any(|scope| {
                !validated.scopes.contains(scope)
                    || form
                        .selected_scopes
                        .iter()
                        .filter(|value| *value == scope)
                        .count()
                        != 1
            }) {
                return Err(oauth_error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_scope",
                    "selected scopes exceed the authorization request",
                ));
            }
            validated
                .scopes
                .retain(|scope| form.selected_scopes.contains(scope));
            let redirect_uri = validated.redirect_uri.as_str().to_owned();
            let code = endpoint
                .oauth
                .authorize(actor, validated)
                .await
                .map_err(unsafe_authorization_error)?;
            Ok(oauth_redirect(
                &endpoint.authorization_server_uri,
                &redirect_uri,
                state_value,
                Some(&code),
                None,
            ))
        }
        _ => Err(oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "authorization decision is invalid",
        )),
    }
}

fn unsafe_authorization_error(error: McpOAuthUseCaseError) -> Response {
    match error {
        McpOAuthUseCaseError::Unavailable => oauth_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "OAuth service is unavailable",
        ),
        _ => oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "authorization request is invalid",
        ),
    }
}

fn safe_authorization_error(
    issuer: &str,
    redirect_uri: &str,
    state: Option<&str>,
    error: McpOAuthUseCaseError,
) -> Response {
    let code = match error {
        McpOAuthUseCaseError::InvalidScope => "invalid_scope",
        McpOAuthUseCaseError::InvalidTarget => "invalid_target",
        McpOAuthUseCaseError::Unavailable => "server_error",
        _ => "invalid_request",
    };
    oauth_redirect_error(issuer, redirect_uri, state, code)
}

fn oauth_redirect_error(
    issuer: &str,
    redirect_uri: &str,
    state: Option<&str>,
    error: &'static str,
) -> Response {
    oauth_redirect(issuer, redirect_uri, state, None, Some(error))
}

fn oauth_redirect(
    issuer: &str,
    redirect_uri: &str,
    state: Option<&str>,
    code: Option<&str>,
    error: Option<&str>,
) -> Response {
    let Ok(mut url) = url::Url::parse(redirect_uri) else {
        return oauth_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "redirect URI is invalid",
        );
    };
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(code) = code {
            pairs.append_pair("code", code);
        }
        if let Some(error) = error {
            pairs.append_pair("error", error);
        }
        if let Some(state) = state {
            pairs.append_pair("state", state);
        }
        pairs.append_pair("iss", issuer);
    }
    Redirect::to(url.as_str()).into_response()
}
