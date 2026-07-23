# REST API（初期境界）

初期公開では通常利用者向けのWeb UIを提供せず、このREST APIを利用境界とする。
同一originの`/acceptance`は実環境の受入確認専用のserver-rendered画面であり、一般的なノート閲覧・編集UIや
管理UIではない。

MCP client向けのBearer token認証・OAuth endpointはCookie REST APIとは分離している。
詳細は[MCP仕様](mcp.md)を参照する。

## 認証

`/api/v1/notes`以下の操作は、OIDC loginまたはroot loginで発行した`marginalis_session` Cookieを必要とする。
sessionがないか、期限切れまたは失効済みなら`401 authentication-required`を返す。

login時には読み取り可能な`marginalis_csrf` Cookieも発行する。`POST`、`PUT`およびlogoutは、
同値を`X-CSRF-Token` headerへ付けなければならない。不在または不一致は`403`で拒否する。
Cookieを伴う変更操作は、これに加えて`Origin`が公開Base URLのoriginと完全一致し、
`Sec-Fetch-Site`が`same-origin`または`none`でなければならない。欠落・不一致・`cross-site`は`403`で
拒否する。reverse proxyの`X-Forwarded-*`はこの判定に使用しない。非browser API clientも同じheaderを
明示して送る。sessionを持たない`POST /auth/root/login`と、Cookieを使わないMCP OAuth token endpointは
このCookie変更policyの対象外である。

通常のログイン開始URLは`GET /auth/oidc/login`である。成功後はBase URLへ戻る。現在のsessionは
`GET /api/v1/session`で確認できる。REST API clientはCookie jarとCSRF cookieを管理する必要がある。
手動受入では`/acceptance`が同一originのHTML formとしてこの処理を補助するが、HTTP statusやheaderを
含むREST契約の確認には外部API clientを使う。

## 稼働状態

`GET /api/v1/health`はprocessのlivenessを返す。OIDC Discovery障害時でもroot緊急ログインを提供するため、
このendpointは`200`を返す。

`GET /api/v1/readiness`は通常利用者のOIDC loginを開始できるかを返す。OIDCが利用可能なら
`200 {"status":"ready","oidc":"available"}`、root-only縮退起動中なら
`503 {"status":"degraded","oidc":"unavailable"}`となる。reverse proxyや監視は、通常利用者向けの
公開可否にこのendpointを用いる。

## OpenAPI契約と互換性

`GET /api/v1/openapi.json`は、ビルド済みbinaryに埋め込まれたOpenAPI 3.1 documentを認証不要で返す。
REST clientはこのdocumentを契約として使用する。Cookie、CSRF、OriginおよびFetch MetadataはHTTP固有の
security schemeであり、MCP tool contractには含めない。

`v0.1.0-rc.2`では、このOpenAPI documentをv0.1.0のfreeze候補とする。RC期間中のfield、status、header、
endpointの破壊的変更は、security、データ破損、ACL漏洩または相互運用性のrelease blocker修正に限る。
v0.1.0のfreeze後は、破壊的変更を新しいAPI version pathへ移し、既存versionでは少なくとも一つの
リリース周期のdeprecation告知とmigration手順を提供する。

## root管理

`POST /auth/root/login`は`{"password":"..."}`を受け取り、rootのパスワードが正しければ
`204`とroot session・CSRF Cookieを返す。失敗時は理由を区別せず`401 authentication-required`を
返す。root sessionは無操作30分または発行から8時間で失効する。root loginはTCP接続元ごとに15分間で
5回の失敗までに制限し、超過時は`429 too-many-requests`を返す。reverse proxyの
`X-Forwarded-For`はこの制限に使わないため、proxyを介する構成ではproxy自体が接続元として扱われる。

| 操作 | endpoint | 成功応答 | 認可 |
| --- | --- | --- | --- |
| 保留OIDCユーザー一覧 | `GET /api/v1/admin/users/pending` | `200`、ユーザー配列 | root |
| 保留OIDCユーザー有効化 | `PUT /api/v1/admin/users/{user_id}/activate` | `204` | root、CSRF |
| 有効OIDCユーザー無効化 | `PUT /api/v1/admin/users/{user_id}/disable` | `204` | root、CSRF |
| 登録ポリシー取得・更新 | `GET`/`PUT /api/v1/admin/registration-policy` | `200`/`204` | root、更新はCSRF |
| MCP client事前登録 | `POST /api/v1/admin/mcp-clients` | `204` | root、CSRF |
| 他ユーザーのMCP認可取消 | `DELETE /api/v1/admin/mcp-authorizations?user_id=...&client_id=...` | `204` | root、CSRF |

有効化は`pending`のOIDCユーザーにだけ作用する。成功後、そのユーザーは次回のOIDC loginで
通常のsessionを得られる。rootのパスワードをHTTP request body以外へ記録・保存してはならない。
初期実装では管理操作はREST APIで提供し、ブラウザー管理UIは後続とする。
root loginと`/api/v1/admin/*`は通常のapplication routerとは独立したmanagement routerへ収容する。
current releaseでは同一listenerでmergeするが、将来の専用管理originまたはmTLS化では、このrouterだけを
別listenerへ載せ替える。

rootのログイン成功・失敗、logout、OIDCユーザーの有効化・無効化、登録ポリシー変更およびrootによる
MCP管理操作はSQLiteの`root_audit_log`へ記録する。監査閲覧用REST APIは設けない。運用者はサーバ上で
DBを直接参照する。password、cookie、session ID、OIDC code、access/refresh tokenとそのhashは記録しない。
記録は日次の`marginalis-prune-audit.timer`が365日より古い行を削除する。同じmaintenanceは、期限切れ・
消費済みのOIDC login attempt、session、削除確認およびMCP tokenを削除するが、ノート正本・投影は対象にしない。

無効化は`active`なOIDCユーザーだけに作用し、同一SQLite transactionで当該ユーザーのWeb session、
MCP access tokenおよびrefresh tokenを失効させる。無効化済みユーザーはOIDC login、REST API、MCPを
利用できない。

登録ポリシーはSQLiteに永続化され、現在は`open`または`approval`を指定できる。更新bodyは
`{"policy":"open"}`または`{"policy":"approval"}`である。`invite-only`は招待機能の導入まで
管理APIから選択できない。

`POST /api/v1/admin/mcp-clients`のbodyは`client_id`、`display_name`、`redirect_uris`を持つJSON objectで
ある。Client ID Metadata Documentを提供しないMCP public clientを明示的に登録するために使う。redirect URIは
HTTPS、またはloopback (`127.0.0.1`、`localhost`、`::1`) のHTTP URIだけを許可し、query、fragment、userinfoを
含めてはならない。

通常ユーザーは`DELETE /api/v1/mcp-authorizations?client_id=...`で自分のclient認可を取り消せる。
rootは`DELETE /api/v1/admin/mcp-authorizations?user_id=...&client_id=...`で任意ユーザーの認可を
取り消せる。いずれもCSRF tokenを必要とし、対象ユーザーとclient IDのaccess tokenおよびrefresh tokenを
すべて直ちに失効させる。

`GET /api/v1/mcp-authorizations`は、現在の通常ユーザーの有効なMCP認可を返す。各要素はclient ID、
表示名、scope、最初の認可時刻（RFC 3339の`authorized_at`）、最後の利用時刻（RFC 3339の`last_used_at`。
未使用なら`null`）を
含む。access token、refresh token、token hashは応答に含めない。

OIDC callbackの成功時はBase URL（`/`）へredirectする。`GET /`はhealth responseを返すため、ログイン完了後に
404にはならない。`/acceptance`は受入確認時にだけ明示して開く。`GET /api/v1/session`は現在の有効なsessionについて
`user_id`と`is_root`を返し、sessionがなければ`401`を返す。

## ノート正本

正本はUTF-8のAsciiDocであり、SQLiteは検索・ACL・参照の投影である。HTTP handlerはSQLiteへ
sourceを直接書き込まない。作成・更新は`NoteWriteService`を呼び、journal、ファイル正本、投影の
順序を調停する。

| 操作 | endpoint | 成功応答 | 認可 |
| --- | --- | --- | --- |
| 一覧 | `GET /api/v1/notes?limit=50&cursor=...` | `200`、可視ノートのID、titleおよび次cursor | 認証済み利用者 |
| 作成 | `POST /api/v1/notes` | `201`、`Location` | 認証済み利用者 |
| metadata取得 | `GET /api/v1/notes/{note_id}` | `200`、ID、title、revision | Read以上 |
| 正本取得 | `GET /api/v1/notes/{note_id}/source` | `200`、AsciiDoc bytesと`ETag` | Read以上 |
| 正本更新 | `PUT /api/v1/notes/{note_id}/source` | `204` | Write以上、`If-Match`必須 |
| 削除準備 | `POST /api/v1/notes/{note_id}/delete-preparations` | `200`、確認tokenと被参照数 | Adminまたはroot、`If-Match`必須 |
| 削除確定 | `POST /api/v1/notes/delete-confirmations` | `204` | tokenを準備したAdminまたはroot |
| ACL更新 | `PUT /api/v1/notes/{note_id}/acl/{user_id}` | `204` | Adminまたはroot |

`GET /api/v1/search?q=<phrase>&limit=50&cursor=...`は、可視なノートだけをSQLite FTS5で検索する。各結果は
`note_id`と`title`を返す。一致箇所の抜粋や本文は返さない。検索語はFTS構文として
解釈せず、一つのphraseとして扱う。一覧・検索とも応答は`notes`と`next_cursor`を持つobjectである。
`tags`は繰返し指定でき、指定したすべてのtagを持つノートに絞り込む。`created_after`、`created_before`、
`updated_after`、`updated_before`にはRFC 3339日時を指定でき、それぞれ境界値を含む。cursorは前の応答の
`next_cursor`だけを受理し、`limit`の既定値は50、上限は100である。作成者・参照方向filterは公開していない。

作成・更新のrequest bodyはAsciiDoc sourceそのものとする。保存前に`marginalis-asciidoc`が、必須
metadata、危険な構文、`xref:note:`、anchor、およびsource言語のprofileを検証して投影を作る。

raw AsciiDoc APIでは、クライアントは文書ヘッダを含むsource全体を送る。`note-id`、`creator-id`、
`created-at`、`updated-at`は各一個のset属性として存在しなければならないが、値は信頼しない。作成時は
serverが新しいUUIDv7、現在のactorおよび同一の作成・更新時刻へ置換する。更新時は`note-id`、
`creator-id`、`created-at`が現行正本と一致する場合だけ受理し、`updated-at`は常にserver時刻へ置換する。
他の属性、文書構造および本文はそのまま保持する。したがって次は作成requestの構造例であり、値を
事前に取得・生成する必要はない。

```adoc
= 研究メモ
:note-id: 01800000-0000-7000-8000-000000000001
:creator-id: 01800000-0000-7000-8000-000000000002
:created-at: 2026-07-23T00:00:00.000Z
:updated-at: 2026-07-23T00:00:00.000Z
:tags: research, idea

xref:note:01800000-0000-7000-8000-000000000003[関連ノート]
```

`POST /api/v1/notes`にはこのUTF-8 sourceをbodyとして送り、`X-CSRF-Token`を付ける。成功時の
`Location`は作成済み正本の`/api/v1/notes/{note_id}/source`である。更新前にはsource取得時の`ETag`を
そのまま`If-Match`へ送る。

`PUT`ではURLの`note_id`と文書内`:note-id:`、既存ノートの`:creator-id:`および`:created-at:`のいずれかを
変更すると`422 validation-failed`を返す。`GET`が返す強い`ETag`を`PUT`または削除準備の`If-Match`へ
そのまま指定する。欠落または現在の
正本revisionとの不一致は`409 conflict`、形式不正は`422 validation-failed`を返す。

削除準備はtitle、revision、5分だけ有効な一回限りの`confirmation_token`、および要求actorから可視な
`incoming_reference_count`を返す。確定request bodyは`{"confirmation_token":"..."}`とする。tokenは
actor、対象、revisionおよび**全**被参照集合へ結び付く。準備後に本文・参照が変化した場合、確定は
`409 conflict`となる。成功時は正本ファイルとSQLite投影・ACL・xref・確認情報を物理削除する。復元API
および保持期間は初期公開には提供しない。

ACL更新のbodyは`{"permission":"read"}`、`"write"`、`"admin"`、または
`{"permission":null}`（直接ACLの解除）である。最後の直接管理者を解除・降格する更新は拒否する。

対象不在、不正なUUIDv7、またはRead/Write権限がない場合は、存在を推測させないため同じ
`404 not-found`を返す。

## Session終了

`POST /auth/logout`は現在のsessionを失効させ、`marginalis_session` Cookieを削除して`204`を返す。
