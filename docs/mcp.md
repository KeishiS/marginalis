# MCP と OAuth

## 概要

Marginalis は、研究ノートの検索・取得・作成・更新・参照一覧・確認付き物理削除のために、
OAuth で保護された MCP（Streamable HTTP）エンドポイントを提供します。

この認可は Web の Cookie セッションとは独立しています。外部の Kanidm は、認可画面で利用者を
ログインさせる OIDC プロバイダーとしてだけ使います。MCP のアクセストークン・リフレッシュ
トークン・認可コードを Kanidm へ渡すことはなく、MCP エンドポイントが Cookie を受け付ける
こともありません。

MCP は `services.marginalis.mcp.enable = true` を設定した場合にだけ有効になります。
ベース URL を `B` とすると、公開 URL は次のとおりです。ベース URL がサブパスを含む場合も、
各パスはそのサブパスの下に置かれます。

| 用途 | URL |
| --- | --- |
| MCP トランスポート | `B/mcp` |
| Protected Resource Metadata | `B/.well-known/oauth-protected-resource/mcp` |
| Authorization Server Metadata | `B/.well-known/oauth-authorization-server` |
| 認可エンドポイント | `B/oauth/authorize` |
| トークンエンドポイント | `B/oauth/token` |

## MCP トランスポート

`POST /mcp` は JSON-RPC 2.0 リクエストを受け付けます。クライアントは `Accept` ヘッダーに
`application/json` と `text/event-stream` の両方を含め、`Authorization: Bearer <access-token>`
を付けてください。Bearer トークンがない、または無効な場合は `401` と次の形式の
`WWW-Authenticate` ヘッダーを返します。

```text
Bearer resource_metadata="https://example.test/.well-known/oauth-protected-resource/mcp"
```

そのほかのトランスポート仕様は次のとおりです。

- `GET /mcp` は、サーバーからクライアントへの通知ストリームが必要になるまで `405` を
  返します。
- リクエストに `Origin` ヘッダーがある場合、ベース URL と同じオリジンでなければ拒否します。
- 認証後のツール呼び出しは利用者ごとに毎分 120 回までです。超過すると `429` と
  `Retry-After: 60` を返します。この制限はメモリ内の固定ウィンドウで、サーバー再起動で
  リセットされます。

## ツール一覧

| ツール | 入力 | 出力 |
| --- | --- | --- |
| `search_notes` | `query`、任意の `limit`・`cursor` | 可視ノートの ID・タイトルと次カーソル |
| `get_note` | `note_id` | Read 権限を持つノートの ID・タイトル・タグ・作成/更新時刻・リビジョン・AsciiDoc 本文 |
| `list_note_links` | `note_id`、任意の `limit`・`cursor` | 参照元・参照先の両方を閲覧できる参照の位置、参照先の ID・タイトル・アンカーと次カーソル |
| `create_note` | `title`、`body`、`tags` | サーバー生成メタデータを持つ新規ノートとリビジョン |
| `update_note` | `note_id`、`expected_revision`、`title`、`body`、`tags` | 更新後のノートとリビジョン |
| `prepare_delete_note` | `note_id`、`expected_revision` | タイトル、リビジョン、一回限りの確認トークン |
| `delete_note` | `confirmation_token` | 物理削除の完了 |

検索は SQLite FTS5 の投影を使い、ACL でフィルターした結果だけをカーソルに含めます。本文の
断片、スコア、権限のないノートの存在は返しません。`list_note_links` も同じ規則に従い、
参照元と参照先の両方に Read 権限がある行だけを返します。参照先が閲覧できない場合は、その
ID・タイトル・アンカー・投影上の存在のいずれも返しません。

### ノートの保護されたメタデータ

`create_note` と `update_note` では、MCP クライアントは `note-id`・`creator-id`・
`created-at`・`updated-at` を指定・変更できません。サーバーが UUIDv7 と作成者を決め、作成
日時を保存し、更新時には更新日時だけを書き換えます。`update_note` は現在のリビジョンとの
完全一致を要求し、競合時には何も変更しません。

### 削除の二段階確認

削除には `notes:delete` スコープとノートの Admin 権限が必要です。`prepare_delete_note` が
返す確認トークンは、実行者・対象ノート・リビジョン・被参照集合に結び付き、5 分で失効します。
結果には実行者から見える範囲の被参照数 `incoming_reference_count` も含まれます。トークンは
`delete_note` で一度だけ消費でき、準備後にリビジョンや被参照状態が変わっていた場合は削除を
行いません。トークンの平文を SQLite へ保存することはありません。

## REST API との対応

REST と MCP のツールは、同じアプリケーション層のユースケース・ACL・リビジョン規則を共有
します。ただしトランスポート固有の認証方式は混在させません。Cookie・`X-CSRF-Token`・
`Origin`・`Sec-Fetch-Site` は REST のブラウザ境界だけの要件であり、MCP のツール入出力には
含まれません。

| REST | MCP | 相違点 |
| --- | --- | --- |
| `GET /api/v1/search` | `search_notes` | REST は Cookie セッション、MCP は Bearer トークンと `notes:read` スコープを使う |
| `GET /api/v1/notes/{note_id}/source` | `get_note` | MCP はメタデータと本文を 1 つの JSON-RPC 結果で返す |
| `POST /api/v1/notes` | `create_note` | REST は AsciiDoc 全文、MCP は `title`/`body`/`tags` の構造化入力を受ける |
| `PUT /api/v1/notes/{note_id}/source` | `update_note` | どちらもリビジョンの完全一致を要求する |
| 削除準備 → 削除確定 | `prepare_delete_note` → `delete_note` | どちらも確認トークンによる二段階削除 |

## OAuth フロー

1. クライアントは Protected Resource Metadata を取得し、リソース URI と認可サーバーを
   特定します。
2. クライアントは `/oauth/authorize` へ `response_type=code`、`client_id`、`redirect_uri`、
   `resource`、`scope`、`code_challenge`、`code_challenge_method=S256`、任意の `state` を
   送ります。
3. 利用者に Cookie セッションがなければ、Marginalis は Kanidm の OIDC ログインへ誘導し、
   認可要求だけを短命の `HttpOnly; Secure; SameSite=Lax` Cookie に保存します。OIDC
   コールバック後は、その認可画面へだけ復帰します。
4. 利用者はクライアント名とスコープを確認し、許可または拒否します。root セッションで MCP
   クライアントを認可することはできません。
5. 許可された場合、完全一致で検証済みのリダイレクト URI へ `code` と元の `state` を付けて
   `303` を返します。
6. クライアントは `POST /oauth/token` へ `grant_type=authorization_code`、コード、
   クライアント ID、リダイレクト URI、リソース、PKCE verifier をフォームボディで送ります。
   成功すると不透明なアクセストークンとリフレッシュトークンを取得できます。レスポンスには
   `token_type: "Bearer"`、`expires_in`、実際に許可された `scope` も含まれます。

制約は次のとおりです。

- `redirect_uri` は事前登録された文字列との完全一致です。HTTPS、またはループバック
  （`127.0.0.1`・`localhost`・`::1`）への HTTP だけを許可します。クエリ・フラグメント・
  ユーザー情報を含む URI は登録できません。
- スコープは `notes:read`・`notes:write`・`notes:delete` だけを受け付けます。クライアントは
  呼び出すツールに対応するスコープを要求してください。スコープに加えて、各操作では通常の
  ノート ACL も検証されます。
- アクセストークンの有効期間は 1 時間です。リフレッシュトークンの有効期間は 30 日で、
  `grant_type=refresh_token` により新しいトークンペアと交換できます。リフレッシュトークンは
  交換と同時に無効化され、再利用は拒否されます。発行済みのアクセストークンは、自身の期限
  または明示的な認可取消まで有効です。

## クライアントの登録

### Client ID Metadata Document

未知のクライアントの `client_id` は HTTPS URL であり、その URL の JSON ドキュメントは
少なくとも次を含みます。

```json
{
    "client_id": "https://clients.example.org/marginalis.json",
    "client_name": "Example MCP client",
    "redirect_uris": ["http://127.0.0.1:4567/callback"]
}
```

Marginalis は、クライアント ID とドキュメント内 `client_id` の完全一致、空でない
`client_name`、空でない `redirect_uris`、各 URI のリダイレクトポリシーを検証してから登録
します。リダイレクトは追跡せず、64 KiB を超える応答は受け付けません。取得先は
`services.marginalis.mcp.clientMetadataAllowedHosts` に列挙した HTTPS ホストに限られます。
これは認可エンドポイントを SSRF の入口にしないための境界です。

Dynamic Client Registration と Device Authorization Grant は現在のリリースには含まれません。

### root による事前登録

Client ID Metadata Document を提供しないクライアントは、root が
`POST /api/v1/admin/mcp-clients` で事前登録できます。この操作には root セッションと CSRF
トークンが必要で、MCP トークンやクライアントシークレットは扱いません。

### 例: ChatGPT のコールバック URL を事前登録する

ChatGPT のカスタム MCP アプリ設定では、登録者が OAuth クライアント ID を指定します。これは
シークレットではないため、安定した値を決めて、ChatGPT 側の「OAuth client ID」と Marginalis
側の事前登録で完全に同じ値を使ってください。root による事前登録では URL 形式は必須では
ないため、`chatgpt-web` のような空でない安定した文字列でも構いません。

- コールバック URL には、ChatGPT の設定画面に表示される完全な値を使います。
- Dynamic Client Registration は公開していないため、ChatGPT の「登録 URL」は空欄にします。
- Marginalis はパブリッククライアントの Authorization Code + PKCE だけを受け付けます。
  `client_secret_basic` と `client_secret_post` は実装していないため、「OAuth client secret」
  も空欄にします。

次の手順は、root セッションと CSRF トークンを取得してクライアントを登録し、最後に root
セッションを破棄します。root のパスワード、Cookie、CSRF トークン、コールバック URL を
コマンド履歴・Issue・ログへ記録しないでください。

```sh
set -eu

BASE_URL='https://marginalis.sandi05.com'
ORIGIN='https://marginalis.sandi05.com'
COOKIE_JAR="$(mktemp)"
trap 'rm -f "$COOKIE_JAR"' EXIT

read -s ROOT_PASSWORD
{
  printf '{"password":'
  printf '%s' "$ROOT_PASSWORD" | jq -Rs .
  printf '}'
} | curl --fail-with-body --silent --show-error \
  --cookie-jar "$COOKIE_JAR" \
  --header 'Content-Type: application/json' \
  --data-binary @- \
  --output /dev/null \
  "$BASE_URL/auth/root/login"
unset ROOT_PASSWORD

CSRF_TOKEN="$(awk '$6 == "marginalis_csrf" { print $7 }' "$COOKIE_JAR")"
[ -n "$CSRF_TOKEN" ]

CHATGPT_CLIENT_ID='chatgpt-web'
read -r CHATGPT_CALLBACK_URL

jq -n \
  --arg client_id "$CHATGPT_CLIENT_ID" \
  --arg callback "$CHATGPT_CALLBACK_URL" \
  '{
    client_id: $client_id,
    display_name: "ChatGPT Marginalis MCP",
    redirect_uris: [$callback]
  }' |
curl --fail-with-body --silent --show-error \
  --cookie "$COOKIE_JAR" \
  --header 'Content-Type: application/json' \
  --header "X-CSRF-Token: $CSRF_TOKEN" \
  --header "Origin: $ORIGIN" \
  --header 'Sec-Fetch-Site: same-origin' \
  --request POST \
  --data-binary @- \
  --output /dev/null \
  --write-out 'MCP client registration: HTTP %{http_code}\n' \
  "$BASE_URL/api/v1/admin/mcp-clients"

curl --fail-with-body --silent --show-error \
  --cookie "$COOKIE_JAR" \
  --header "X-CSRF-Token: $CSRF_TOKEN" \
  --header "Origin: $ORIGIN" \
  --header 'Sec-Fetch-Site: same-origin' \
  --request POST \
  --output /dev/null \
  "$BASE_URL/auth/logout"
```

成功すると `HTTP 204` になります。コールバック URL は HTTPS で、クエリ・フラグメント・
ユーザー情報を含まず、ChatGPT が送る `redirect_uri` と完全に一致する必要があります。

## 認可の確認と取消

- 利用者は `GET /api/v1/mcp-authorizations` で、自分の有効なクライアント認可の表示名・
  スコープ・認可日時・最終利用日時を確認できます。応答にトークンやそのハッシュは含まれない
  ため、この一覧から安全に取消対象を選べます。
- 利用者は REST API で自分のクライアント認可を取り消せます。root は任意ユーザーの認可を
  強制的に取り消せます。取消時には、そのユーザーとクライアントの組に発行済みのアクセス
  トークンとリフレッシュトークンをすべて失効させます。
