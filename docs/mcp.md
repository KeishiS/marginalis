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
  redirect URIの登録が1件だけなら、認可要求では`redirect_uri`を省略できます。認可要求で指定した
  場合は、token要求にも同じ値を指定する必要があります。
- MCP endpointと一致する`resource`。
- 対応しているscopeだけから成る`scope`。操作との対応は次表のとおりです。
- 43文字のS256 challengeと、43文字以上128文字以下のPKCE verifier。
- 一度だけ使える認可codeとrefresh token。

| scope | 許可する操作 |
|---|---|
| `notes:read` | ノートの一覧・本文の取得、ノート記述規則の取得 |
| `notes:write` | ノートの作成・更新、ノート記述規則の取得 |
| `notes:delete` | ノートの削除 |
| `bibliography:read` | 書誌情報の検索 |
| `bibliography:write` | 書誌情報の追加 |
| `bibliography:delete` | 書誌情報の削除 |

ノート用scopeは書誌情報の操作を許可せず、書誌用scopeもノートの操作を許可しません。たとえば、
`notes:read`だけを持つtokenで`search_bibliography`を呼び出すと、`bibliography:read`を示す
`403 insufficient_scope`を返します。

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
`client_id`が取得元URLと完全に一致することを検査します。client認証方式は、単一の方式を示す
`token_endpoint_auth_method`と、利用できる方式の一覧を示す
`token_endpoint_auth_methods_supported`の両方を解釈します。両方が矛盾せず、Marginalisが対応する
`none`を選べる場合だけpublic clientとして受理します。`private_key_jwt`は使用しません。

`client_id`のURLは、HTTPSであることと、空でないpathを持つことを必要とします。userinfo、query、
fragment、`.`や`..`のようなドット区間を含むURLは受け付けません。運用条件を単純に保つための制限です。

外部文書の取得では、HTTP redirect、特別用途IPアドレス、5 KiBを超える応答を拒否します。有効な文書は
`Cache-Control`と`Age`に従って最大1時間保持します。取得の失敗や不正な文書はCIMDの仕様に従って保持
しません。同じ`client_id`への同時取得は一つにまとめ、外部取得は全体で同時に8件、1分あたり60回までに
制限します。DNSの名前解決を含む1回の取得は5秒で打ち切ります。上限に達した場合は、clientが不正である
とは伝えず、一時的に利用できないものとして扱います。

Dynamic Client Registration（DCR）にも対応します。DCRはMCP仕様`2026-07-28`では非推奨ですが、
既存クライアントとの互換経路として残します。登録数は1,000件に制限し、同じredirect originからの
登録要求にも時間当たりの上限を設けます。上限に達した場合は`503 temporarily_unavailable`を返します。
client metadataの誤りではなく、サーバー側に空きがない状態だからです。CIMDで保存したclientはDCRの
登録数へ含めません。

CIMDのclientは事前登録しません。利用者が同意した時点で、そのときのclient名とredirect URIを認可code
と一緒に保存します。期限切れの認証状態と、24時間以上参照されていない古いclientは、日次の
`purge-expired`で削除します。

## 接続

クライアントには公開MCP URLだけを設定します。例は`https://notes.example.test/mcp`です。クライアントは
Protected Resource Metadataから内蔵Authorization Serverを発見し、ブラウザーでKanidmへログインした後、
要求されたscopeへ同意します。

更新前にAuth0で発行したaccess token、refresh token、client IDは内蔵Authorization Serverへ移行されません。
更新後はクライアント側の既存接続を削除し、改めて接続してください。

v0.29.0より前に発行したtokenには書誌用scopeが含まれません。MCPから書誌情報を操作する場合は、
クライアントを改めて認可し、必要な`bibliography:*` scopeへ同意してください。既存tokenへ新しいscopeが
自動で追加されることはありません。

### 接続後の受入

ChatGPT、Claude Code、Codex CLIごとに、同じ利用者と試験用ノートを使って次を確認します。

1. 公開MCP URLだけを指定して接続し、metadataの発見、client登録、Kanidmへのログイン、同意を完了します。
2. `tools/list`を取得し、ノートの一覧と本文を読み取ります。
3. 試験用ノートを作成して更新し、別の試験利用者へ閲覧権限と編集権限を順に設定します。共有先では
   付与された権限を超える操作が拒否されることも確認します。
4. 試験用ノートを削除し、削除済みとして取得できることを確認します。
5. 認可を取り消し、既存のaccess tokenとrefresh tokenが使えないことを確認します。
6. 改めて認可し、同じクライアントから接続を回復できることを確認します。

結果には確認日、クライアント名と確認できる範囲の版、CIMDまたはDCRの登録方式、redirect URIの種類を
記録します。token、認可code、PKCE verifier、Cookie、実際のノート本文は記録しません。

## 認可の取消

取消の経路は二つです。

利用者本人は`DELETE /api/v3/mcp-authorizations/{client_id}`でclient単位に取り消します。Web UIと同じ
session cookieとCSRF tokenが必要で、取り消せるのは自分に発行された認可だけです。現時点でこの操作を
行う画面はないため、REST APIとして使います。

OAuthクライアントはRFC 7009の`/oauth/revoke`へtokenと`client_id`を送信します。未知のtokenを指定した
場合も、tokenの存在を開示しないため成功応答を返します。

どちらの経路でも、対象のaccess tokenとrefresh tokenをtoken family単位で直ちに失効します。

## 失敗した場合の応答

MCP endpointでは、tokenがない場合と不正な場合に`401`と`WWW-Authenticate`を返します。必要なscopeが
ない場合は`403 insufficient_scope`です。SQLiteを利用できない場合は`503`として扱います。

OAuth endpointはOAuthの`error`を持つJSONを返します。tokenを含む応答には`Cache-Control: no-store`を
付けます。token、認可code、PKCE verifier、session cookieはログへ出力しません。

運用時は`marginalis diagnose`でSQLite schemaとMCP有効化設定を確認してください。認証状態の削除件数は
`marginalis purge-expired`の構造化ログで確認できます。CIMDの取得と検証を調べる場合は、
[ログと障害診断](observability.md)に記載した`mcp.oauth.client_metadata.*` eventを確認してください。
