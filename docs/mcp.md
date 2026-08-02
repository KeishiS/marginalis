# MCPへの接続と認可

この文書は、MCPクライアントをMarginalisへ接続する人と運用者に向けて、内蔵Authorization
Serverによる認可と失敗時の確認方法を説明します。

## 構成

MarginalisはMCPのProtected ResourceとAuthorization Serverを同じ公開base URLで提供します。
利用者のログインにはWeb UIと同じKanidm OIDCを使い、`server-users`に所属する利用者だけが同意画面へ
進めます。外部Authorization Serverは使用しません。

MCPを有効にすると、主に次のendpointを公開します。

| 用途 | endpoint |
|---|---|
| Protected Resource Metadata | `/.well-known/oauth-protected-resource/mcp` |
| Authorization Server Metadata | `/.well-known/oauth-authorization-server` |
| 認可 | `/oauth/authorize` |
| token発行・更新 | `/oauth/token` |
| token失効 | `/oauth/revoke` |
| 動的なclient登録 | `/oauth/register` |
| MCP | `/mcp` |

`baseUrl`にサブパスがある場合、well-known URLはRFC 8414とRFC 9728に従ってサブパスを保持します。

## 認可方式

認可にはAuthorization CodeとPKCE S256を使います。Implicit Grant、Resource Owner Password
Credentials Grant、client secretによる公開clientの認証には対応しません。

Authorization Serverは次を検査します。

- 登録済みの`client_id`と`redirect_uri`の完全一致。loopback URIでは接続ごとのport変更だけを許可します。
- MCP endpointと一致する`resource`。
- `notes:read`、`notes:write`、`notes:delete`のいずれかから成る`scope`。
- 43文字のS256 challengeと、43文字以上128文字以下のPKCE verifier。
- 一度だけ使える認可codeとrefresh token。

認可の成功応答と、検証済み`redirect_uri`へ返すエラー応答にはRFC 9207の`iss`を含めます。クライアントは
Authorization Server Metadataの`issuer`と単純な文字列比較を行うことで、別のAuthorization Serverから
応答を差し替える攻撃を検出できます。

access tokenとrefresh tokenは推測困難な不透明tokenです。SQLiteにはtokenそのものではなくSHA-256
hashだけを保存します。access tokenの有効期間は1時間、refresh tokenの有効期間は30日です。
refresh tokenは利用するたびに交換し、使用済みtokenが再利用された場合は同じtoken familyをすべて
失効します。

## client登録

Client ID Metadata Document（CIMD）を優先方式として提供します。CIMDは、HTTPS URLを`client_id`として
使い、そのURLでclient名と`redirect_uris`を含むJSON文書を公開する方式です。Marginalisは文書内の
`client_id`が取得元URLと完全に一致することと、認証方式が`none`であることを検査します。

外部文書の取得では、HTTP redirect、特別用途IPアドレス、5 KiBを超える応答を拒否します。URLには
HTTPS、path、userinfo・fragment・ドット区間を含まないことを要求します。queryを含むURLも、安全で
単純な運用条件にするため受け付けません。有効な文書は`Cache-Control`と`Age`に従って最大1時間保持し、
エラー応答や不正な文書は保持しません。

Dynamic Client Registration（DCR）にも対応します。DCRはMCP仕様`2026-07-28`では非推奨ですが、
既存クライアントとの互換経路として残します。登録数は1,000件に制限し、同じredirect originからの
登録要求にも時間当たりの上限を設けます。期限切れの認証状態と参照されていない古いclientは、日次の
`purge-expired`で削除します。

## 接続

クライアントには公開MCP URLだけを設定します。例は`https://notes.example.test/mcp`です。クライアントは
Protected Resource Metadataから内蔵Authorization Serverを発見し、ブラウザーでKanidmへログインした後、
要求されたscopeへ同意します。

更新前にAuth0で発行したaccess token、refresh token、client IDは内蔵Authorization Serverへ移行されません。
更新後はクライアント側の既存接続を削除し、改めて接続してください。

## 認可の取消

Web UIから接続単位で認可を取り消せます。また、OAuthクライアントはRFC 7009の`/oauth/revoke`へtokenと
`client_id`を送信できます。どちらの操作でも対象のaccess tokenとrefresh tokenをtoken family単位で
直ちに失効します。未知のtokenを指定した場合も、tokenの存在を開示しないため成功応答を返します。

## 失敗した場合の応答

MCP endpointでは、tokenがない場合と不正な場合に`401`と`WWW-Authenticate`を返します。必要なscopeが
ない場合は`403 insufficient_scope`です。SQLiteを利用できない場合は`503`として扱います。

OAuth endpointはOAuthの`error`を持つJSONを返します。tokenを含む応答には`Cache-Control: no-store`を
付けます。token、認可code、PKCE verifier、session cookieはログへ出力しません。

運用時は`marginalis diagnose`でSQLite schemaとMCP有効化設定を確認してください。認証状態の削除件数は
`marginalis purge-expired`の構造化ログで確認できます。
