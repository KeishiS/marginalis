# MCP と OAuth

Marginalis は MCP の OAuth Authorization Server と Protected Resource を同一オリジンで提供します。
Kanidm token を MCP client に渡すことはありません。

| 対象 | endpoint |
| --- | --- |
| MCP | `POST B/mcp` |
| Protected Resource Metadata | RFC 9728で`B/mcp`から導出するURL |
| Authorization Server Metadata | RFC 8414で`B`から導出するURL |
| Dynamic Client Registration | `POST B/oauth/register` |
| Authorization開始 | `GET` / `POST B/oauth/authorize` |
| Marginalis承認確定 | `POST B/oauth/authorize/consent` |
| Token | `POST B/oauth/token` |

ここで `B` は外部 base URL です。クライアントは Dynamic Client Registration を行い、Authorization
Code + PKCE S256 を使います。未ログインで authorization endpoint を開いた場合は OIDC login へ移動し、
認可リクエストへ安全に戻ります。

OAuth clientからの認可開始はquery付き`GET`とform-encoded `POST`の両方を受け付けます。POSTのOAuth
parameterはURL queryとform bodyのどちらにあってもよいですが、同じparameterを複数回送ると値が同じでも
`invalid_request`として拒否します。空のparameterは省略として扱い、未知のparameterは無視します。
ChatGPTやClaudeがclient originから送る初回POSTにclient自身のCSRF fieldが含まれていても、
`B/oauth/authorize`は登録済みclient、redirect URI、resource、scope、PKCEを検証するだけで認可を
確定しません。未ログイン時は`303 See Other`で`GET`のOIDC loginへ移動します。

ログイン後にMarginalisが表示する承認formだけが`B/oauth/authorize/consent`へPOSTし、認可を作成します。
OAuth clientのpopupやsandboxでは`Origin`が欠落またはopaqueになり得るため、このendpointは
同一session、CSRF cookie、Marginalisが発行してsessionへ紐付けたform tokenの一致を必須とします。
外部clientの認可開始endpointと状態変更endpointを分け、field名による推測では分類しません。

well-known suffixはhostとsubject pathの間へ挿入します。base URLがhost rootかsubpathかで
URLが次のように変わります。

- `B = https://notes.example.test`: Protected Resource Metadataは
  `https://notes.example.test/.well-known/oauth-protected-resource/mcp`、Authorization Server
  Metadataは`https://notes.example.test/.well-known/oauth-authorization-server`。
- `B = https://notes.example.test/marginalis`: Protected Resource Metadataは
  `https://notes.example.test/.well-known/oauth-protected-resource/marginalis/mcp`、Authorization
  Server Metadataは
  `https://notes.example.test/.well-known/oauth-authorization-server/marginalis`。

これはRFC 9728とRFC 8414のpath付きsubject規則です。KanidmのOIDC `issuerUrl`は外部Identity
ProviderのURLであり、Marginalis自身のOAuth metadata URLの導出には使いません。

`/mcp` は Cookie を使わず、すべての request を `Authorization: Bearer` で認可します。`Origin` がある
browser client は DNS rebinding 対策として完全一致の許可リストで検証します。NixOS module の既定値は
空であり、ChatGPT Web UIを使う場合は`https://chatgpt.com`を明示します。Codex CLI と Claude Code の
ように `Origin` を送らない native client はこの制約の対象外です。

Claude Codeは次のようにremote Streamable HTTP serverとして追加し、Claude Code内の`/mcp`から
browser認証します。

```bash
claude mcp add --transport http marginalis https://marginalis.sandi05.com/mcp
```

Dynamic Client RegistrationではClaude Codeの`http://localhost:PORT/callback`を受け付けます。
HTTP callbackは`localhost`完全一致、またはloopback IP addressだけを許可し、認可要求時の動的なportを
登録値と異なる値でも受け付けます。hostとport以外の部分は登録値との完全一致を維持します。HTTPS callbackは
登録値と完全一致しなければなりません。SSH、container、WSL上のClaude Codeではbrowserからcallback
listenerへ到達できる構成が別途必要です。
このnative client profileはHTTPSとloopback HTTPだけを対象とし、private-use URI schemeは提供しません。
一般的なnative application全体ではなく、loopback callbackを使う受入対象clientとの相互運用に限定します。
RFC 8252がIP literalを推奨する一方、`localhost`はClaude Code互換性のために許容し、承認画面でlocal
applicationへのredirectであることを明示します。

Claude.aiのWeb UIでは、`Customize`の`Connectors`からcustom connectorとして
`https://marginalis.sandi05.com/mcp`を追加します。この接続はAnthropicのcloudから行われ、OAuthの
browser loginと承認を経ます。Claude.ai subscriptionでClaude Codeへログインしている場合は、Claude.aiで
追加したconnectorがClaude Codeにも表示されます。API key、Amazon Bedrock、Google Vertex AIで認証した
Claude CodeにはClaude.ai側のconnectorは同期されないため、上記の`claude mcp add`を使います。

この許可リストは MCP transport 専用です。OAuth の承認画面は Marginalis が表示する Authorization Server
との操作ですが、clientのpopupやsandboxに依存しないよう`Origin`を認可根拠にはしません。
`/oauth/authorize/consent`はsession-bound CSRF tokenを必須とします。client originから
`/oauth/authorize`へ送る認可開始POSTは状態変更を一切行いません。

Authorization Server は登録済み client、redirect URI、MCP resource URI、scope、PKCE S256 を login 前と
承認時の両方で検証します。承認画面には登録済み client 名、要求 scope、redirect host を表示します。
認可要求でscopeを省略した場合は最小権限の`notes:read`を使います。登録redirect URIが一つだけなら認可要求と
token交換の`redirect_uri`は省略できます。token交換で指定した場合は、認可時の値と一致しなければなりません。
access token は 1 時間、rotation される refresh token は 30 日有効です。
refresh時のscopeは元のgrantの部分集合だけを許可し、発行するaccess tokenをdownscopeできます。
使用済み refresh token が正しい client と resource の組合せで再提示された場合は replay と判定し、
同じ token family の access token と refresh token をすべて失効させます。利用者は再度認可してください。
使用済み認可codeが同じclient、resource、PKCE bindingで再提示された場合も、同じtoken familyを失効させます。
認可codeの有効期限後も、対応するtoken familyが残る間はreplay検知情報を保持します。
rotation の親子関係も、有効な子孫がある間保持します。これは
[OAuth 2.0 Security Best Current Practice §4.14.2](https://www.rfc-editor.org/rfc/rfc9700.html#section-4.14.2)
の replay 検知要件に従うものです。
現行のschema versionは4です。旧schemaのdatabaseは起動時に移行せず拒否します。空の現行databaseで
再初期化し、MCP clientは再登録・再認可してください。

Dynamic Client Registration は 16 KiB の本文上限、redirect originごとに10分あたり30件のrate limit、
最大1,000 clientの永続化上限を持ちます。grantを取得しない登録は24時間後の日次保守で削除します。登録・token
endpointが受理したprotocol/application errorはOAuthの`error` / `error_description`形式で返します。
本文上限超過、MCP無効時のroutingなどhandler外のHTTP境界の失敗はこの形式を保証しません。
MCP requestでは無効または失効済みtokenを`401 invalid_token`、必要scopeを持たない有効なtokenを
`403 insufficient_scope`として区別し、`WWW-Authenticate`にProtected Resource Metadata URLと必要scopeを
含めます。
public client専用token endpointでHTTP認証を試みた場合は`401 invalid_client`と、提示された認証schemeの
`WWW-Authenticate`を返します。

MCP transportは[JSON-RPC 2.0](https://www.jsonrpc.org/specification)の`jsonrpc`、method、params、idを
厳密に検証します。[MCP base protocol](https://modelcontextprotocol.io/specification/2025-11-25/basic)
の上乗せ仕様に従い、request IDは文字列または整数だけを許可し、`null`、Boolean、小数を拒否します。
[Streamable HTTP](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)のPOST bodyは
単一messageだけを許可するためbatchは受理しません。parse errorは`-32700`、不正なrequestは`-32600`、
不明methodは`-32601`、不正paramsは`-32602`です。tool実行時の業務エラーはJSON-RPC errorではなくMCP
[tool result](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)の`isError: true`で返します。
`tools/call.arguments`は省略時に空objectとして扱い、`structuredContent`は常にobjectで返します。`ping`にも
空objectで応答します。
[MCP lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)に従い、初期化時は
`2025-11-25`と`2025-03-26`をnegotiationし、以後の`MCP-Protocol-Version`が未知ならHTTP 400で拒否します。
現行transportは`MCP-Session-Id`を発行しないstateless構成であり、clientは初期化順序と交渉したversionを
保持します。serverは各requestのprotocol headerを検証します。

## ノートtoolと入力診断

| tool | scope | 用途 |
| --- | --- | --- |
| `get_note_profile` | `notes:read`または`notes:write` | 現行の入力制約、禁止規則、許可言語、動作例の取得 |
| `list_notes` | `notes:read` | 可視ノートの一覧 |
| `get_note` | `notes:read` | 可視ノートの取得 |
| `create_note` | `notes:write` | ノートの作成 |
| `update_note` | `notes:write` | revisionを指定した更新 |
| `delete_note` | `notes:delete` | revisionを指定したソフトデリート |

`create_note`または`update_note`の前に`get_note_profile`を呼び出してください。profileには
AdocWeave package版`0.11.0`とMarginalis note profile版`1`を別々に含めます。相対link、文書間xref、scheme付きxref、
include、passthroughおよび外部Resourceは現行profileでは保存できません。ローカルanchorへの参照は
利用できます。AdocWeave 0.11.0で追加された`asciidoc-file-link`と`non-asciidoc-xref`は既定の
警告として有効ですが、現行profileの保存可否は変更しません。`macro-boundary`は任意規則のため
有効化しません。

JSONまたはtool引数の構造が不正な場合はJSON-RPC `-32602`です。構造が正しく、ノート規則に違反する場合は
次のようにtool実行結果で返します。`span`は利用者が送った`body`を基準とするUTF-8 byteの半開区間です。
タイトルやタグなど本文位置を持たない診断では`span`を省略します。`content`のtextには
`structuredContent`と同じJSONを直列化して返します。

```json
{
  "content": [
    {
      "type": "text",
      "text": "{\"code\":\"validation_failed\",\"message\":\"note input is invalid\",\"diagnostics\":[{\"code\":\"unsupported_source_language\",\"target\":{\"field\":\"body\"},\"span\":{\"start\":8,\"end\":17,\"unit\":\"utf8_byte\"},\"message\":\"the source block language is not allowed\"}]}"
    }
  ],
  "structuredContent": {
    "code": "validation_failed",
    "message": "note input is invalid",
    "diagnostics": [
      {
        "code": "unsupported_source_language",
        "target": { "field": "body" },
        "span": { "start": 8, "end": 17, "unit": "utf8_byte" },
        "message": "the source block language is not allowed"
      }
    ]
  },
  "isError": true
}
```

本リリースのChatGPT、Claude、Codex受入では、互換登録経路としてDynamic Client Registrationを使用します。
対象clientごとの成否を記録するまで未検証として扱います。MCP 2025-11-25が推奨（SHOULD）する
Client ID Metadata Documentには意図的に対応しません。client指定URLをAuthorization Serverから取得する
方式にはSSRF、名前解決変更、取得制限、cacheの対策が必要であり、受入対象のDCR経路に不要なoutbound HTTP
依存を増やすためです。対象clientがDCRを廃止した場合は、この判断を再検討します。

OAuth endpointへ一律のCORSは付与しません。authorization endpointはnavigation/form送信を受け、CORSを
提供しません。token交換、Dynamic Client Registration、MCP requestはclient backendまたはnative client
から行うことを受入試験で確認します。browser内JavaScriptから直接呼び出す汎用clientは対象外です。

scope は `notes:read`、`notes:write`、`notes:delete` です。scopeは許可する操作を制限しますが、
所有範囲を拡張しません。通常利用者は自身が作成したノートだけを操作でき、`server-admins`は
すべてのノートを操作できます。

利用者は Web session と CSRF token を使って、`DELETE /api/v2/mcp-authorizations/{client_id}` から
個別 client の認可を取り消せます。取り消し後、その client の access token と refresh token は使えません。

OIDC login 時に検証した `groups` claim を Web session と MCP authorization の権限スナップショットとします。
group 変更は次回 login から反映され、既存 token は有効期限または認可取消まで発行時の権限を保持します。
