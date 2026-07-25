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
browser client は DNS rebinding 対策として完全一致の許可リストで検証し、NixOS module の既定値は
`https://chatgpt.com` です。Codex CLI と Claude Code のように `Origin` を送らない native client はこの
制約の対象外です。

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
rotation の親子関係は、有効な子孫がある間保持します。これは
[OAuth 2.0 Security Best Current Practice §4.14.2](https://www.rfc-editor.org/rfc/rfc9700.html#section-4.14.2)
の replay 検知要件に従うものです。
現行のschema versionは2です。旧schemaのdatabaseは起動時に移行せず拒否します。空の現行databaseで
再初期化し、MCP clientは再登録・再認可してください。

Dynamic Client Registration は 16 KiB の本文上限、10 分あたり 30 件の process 全体 rate limit、最大 1,000
client の永続化上限を持ちます。grant を取得しない登録は 24 時間後、次の登録処理時に削除します。登録・token
endpoint の失敗は OAuth の `error` / `error_description` 形式で返します。MCP requestでは無効または
失効済みtokenを`401 invalid_token`、必要scopeを持たない有効なtokenを`403 insufficient_scope`として
区別し、`WWW-Authenticate`にProtected Resource Metadata URLと必要scopeを含めます。

scope は `notes:read`、`notes:write`、`notes:delete` です。scope だけでは不十分であり、Web と同じ
ノート ACL が必ず適用されます。`server-admins` はすべてのノートに管理者相当でアクセスします。

利用者は Web session と CSRF token を使って、`DELETE /api/v2/mcp-authorizations/{client_id}` から
個別 client の認可を取り消せます。取り消し後、その client の access token と refresh token は使えません。

OIDC login 時に検証した `groups` claim を Web session と MCP authorization の権限スナップショットとします。
group 変更は次回 login から反映され、既存 token は有効期限または認可取消まで発行時の権限を保持します。
