# MCPとOAuth

## 範囲

Marginalisは、研究ノートの検索と取得のためにOAuth保護されたMCP Streamable HTTP endpointを提供する。
この認可はWebのCookie sessionとは別であり、外部Kanidmは認可画面で利用者をログインさせるOIDC
providerとしてのみ使う。MCP access token、refresh token、authorization codeをKanidmへ渡したり、
CookieをMCP endpointで受理したりしない。

MCPは`services.marginalis.mcp.enable = true;`を設定した場合だけ有効になる。Base URLを`B`とすると、
公開URLは次のとおりである。

| 用途 | URL |
| --- | --- |
| MCP transport | `B/mcp` |
| Protected Resource Metadata | `B/.well-known/oauth-protected-resource/mcp` |
| Authorization Server Metadata | `B/.well-known/oauth-authorization-server` |
| authorization endpoint | `B/oauth/authorize` |
| token endpoint | `B/oauth/token` |

Base URLがsubpathを持つ場合も、上表のpathはそのsubpathの下へ追加される。

## MCP transport

`POST /mcp`はJSON-RPC 2.0 requestを受ける。clientは`Accept`に`application/json`と
`text/event-stream`の両方を含め、`Authorization: Bearer <access-token>`を付ける。Bearer tokenが
ないか無効なら、serverは`401`と次の形式の`WWW-Authenticate`を返す。

```text
Bearer resource_metadata="https://example.test/.well-known/oauth-protected-resource/mcp"
```

初期toolは次の二つである。

| tool | 入力 | 出力 |
| --- | --- | --- |
| `search_notes` | `query`、任意の`limit`と`cursor` | 可視ノートのID・titleと次cursor |
| `get_note` | `note_id` | Read権限を持つノートのID・title・tags・作成/更新時刻・revision・AsciiDoc source |
| `create_note` | `title`、`body`、`tags` | server生成metadataを持つ新規ノートとrevision |
| `update_note` | `note_id`、`expected_revision`、`title`、`body`、`tags` | 更新後のノートとrevision |
| `prepare_delete_note` | `note_id`、`expected_revision` | title、revision、一回限りの確認token |
| `delete_note` | `confirmation_token` | 物理削除の完了 |

検索はSQLite FTS5投影を使い、ACL filter後の結果だけをcursorへ含める。本文断片、score、権限のない
ノートの存在は返さない。`GET /mcp`はserver-to-client notification streamが必要になるまで`405`を
返す。MCP requestに`Origin` headerが存在するときは、Base URLと同じoriginでなければ拒否する。
認証後のtool呼出しは利用者ごとに毎分120回までであり、超過時は`429`と`Retry-After: 60`を返す。
この制限はメモリ内の固定windowであり、server再起動でリセットされる。

## OAuth flow

1. clientはProtected Resource Metadataを取得し、resource URIとAuthorization Serverを知る。
2. clientは`/oauth/authorize`へ`response_type=code`、`client_id`、`redirect_uri`、`resource`、
   `scope`、`code_challenge`、`code_challenge_method=S256`、任意の`state`を送る。
3. 利用者にCookie sessionがなければ、MarginalisはKanidmのOIDC loginへ移動し、認可要求だけを短命の
   `HttpOnly; Secure; SameSite=Lax` cookieへ保存する。OIDC callback後はその認可画面へだけ復帰する。
4. 通常ユーザーはclient名、scopeを確認して許可または拒否する。root sessionはMCP clientを認可できない。
5. 許可時は完全一致で検証したredirect URIへ`code`と元の`state`を付けて`303`する。
6. clientは`POST /oauth/token`に`grant_type=authorization_code`、code、client ID、redirect URI、
   resource、PKCE verifierをform bodyで送る。成功時にopaque access tokenとrefresh tokenを得る。
   token responseには`token_type: "Bearer"`、`expires_in`、実際に許可された`scope`も含まれる。

`redirect_uri`は事前に登録された文字列との完全一致である。HTTPS URI、または`127.0.0.1`、`localhost`、
`::1`へのHTTP loopback URIだけを許可する。query、fragment、userinfoを含むredirect URIは許可しない。
scopeは`notes:read`、`notes:write`、`notes:delete`だけを受理する。現在公開するtoolは読み取り専用のため、
clientは`notes:read`を要求する。

`create_note`と`update_note`は`notes:write`に加えて、通常のノートACLを必要とする。MCP clientは
`note-id`、`creator-id`、`created-at`、`updated-at`を指定・変更できない。serverがUUIDv7と作成者を
決め、作成日時を保存し、更新日時だけを更新する。`update_note`は現在のrevisionとの完全一致を要求し、
競合時には変更しない。

削除toolは`notes:delete`とAdmin ACLを必要とする。`prepare_delete_note`が返す確認tokenは実行者、
対象ノート、revisionへ結び付けられ、5分で失効する。`delete_note`で一度だけ消費され、確認後に
revisionまたはACLが変わっていれば物理削除を行わない。tokenの平文はSQLiteへ保存しない。

access tokenの有効期間は1時間である。refresh tokenの有効期間は30日で、`grant_type=refresh_token`、
`refresh_token`、client ID、resourceを`/oauth/token`へ送ると新しいtoken pairを得る。refresh tokenは
交換と同時に無効化され、再利用は拒否される。

## Client ID Metadata Document

未知clientの`client_id`はHTTPS URLであり、そのURLのJSON documentは少なくとも次を持つ。

```json
{
  "client_id": "https://clients.example.org/marginalis.json",
  "client_name": "Example MCP client",
  "redirect_uris": ["http://127.0.0.1:4567/callback"]
}
```

Marginalisはclient IDとdocument中の`client_id`の完全一致、空でない`client_name`、非空の
`redirect_uris`、各URIのredirect policyを検証してから登録する。redirectを追跡せず、応答は64 KiBを
超えない。未知clientのmetadata取得は`services.marginalis.mcp.clientMetadataAllowedHosts`に含まれる
HTTPS hostだけに限る。これは任意URLを取得するSSRFを防ぐ運用上の境界である。

Dynamic Client RegistrationとDevice Authorization Grantは初期公開に含めない。

Client ID Metadata Documentを提供しないclientは、rootが`POST /api/v1/admin/mcp-clients`を使って事前登録
できる。これはroot sessionとCSRFを必要とし、MCP tokenやclient secretは受け取らない。
