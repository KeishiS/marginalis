//! MCP OAuth同意画面のHTML生成と、表示した要求を守る署名。
//!
//! 認可プロトコルの判断は`authorization`が行い、このファイルは画面の組み立てだけを持つ。
//! 隠しfieldと`consent_signature`は、表示した内容と同意POSTの内容が一致することを保証する。

use axum::response::{Html, IntoResponse, Response};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use marginalis_application::McpValidatedAuthorizationRequest;
use sha2::Sha256;

use super::super::{
    auth::external_path,
    html::{escape_html, page_document},
    state::ApiState,
};
use super::authorization::McpAuthorizeInput;

pub(super) fn consent_page(
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
    // 同意画面は主要な移動先のどれでもないため、現在位置を示さない。
    Html(page_document(
        "MCPクライアントの認可",
        &state.cookie_path,
        "/oauth/authorize",
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
        "notes:sync" => concat!(
            "sync_notesで、閲覧できるノートの本文と変更を外部の検索用コピーへ継続的に同期します。",
            "許可を取り消しても、Marginalisから外部に保存済みのコピーは削除できません。"
        ),
        "bibliography:read" => "search_bibliographyで書誌情報を検索します。",
        "bibliography:write" => "add_bibliography_itemで書誌情報を一項目ずつ追加します。",
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

pub(super) fn verify_consent_signature(
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
