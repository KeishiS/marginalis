# REST API リファレンス

現在は一般利用者向けの Web UI を提供していないため、REST API が主要な利用インターフェース
です。同一オリジンの `/acceptance` は受入確認専用であり、ノートの閲覧・編集画面では
ありません。

MCP クライアント向けの Bearer トークン認証と OAuth エンドポイントは、この Cookie ベースの
REST API とは分離されています。詳細は [MCP と OAuth](mcp.md) を参照してください。

## 認証

保護された `/api/v1` の操作には、OIDC ログインまたは root ログインで発行される
`marginalis_session` Cookie が必要です。セッションがない、期限切れ、または失効済みの場合は
`401 authentication-required` を返します。

- ログイン開始 URL は `GET /auth/oidc/login` です。成功するとベース URL へ戻ります。
- 現在のセッションは `GET /api/v1/session` で確認できます。有効なセッションがあれば
  `user_id` と `is_root` を返し、なければ `401` を返します。
- OIDC コールバック成功後は `/` へリダイレクトします。`GET /` はヘルスレスポンスを返すため、
  ログイン直後に 404 にはなりません。

### CSRF 保護

ログイン時には、JavaScript から読み取り可能な `marginalis_csrf` Cookie も発行します。
`POST`・`PUT`・ログアウトなど Cookie を伴う変更操作では、次の条件を満たす必要があります。
満たさないリクエストは `403` で拒否します。

1. `X-CSRF-Token` ヘッダーに `marginalis_csrf` と同じ値を付ける。
2. `Origin` ヘッダーが公開ベース URL のオリジンと完全に一致する。
3. `Sec-Fetch-Site` ヘッダーが送られた場合は、値が `same-origin` または `none` である。

リバースプロキシが付与する `X-Forwarded-*` はこの判定に使いません。ブラウザー以外の API
クライアントも、CSRF トークンと `Origin` を明示的に送ってください。`Sec-Fetch-Site` は
対応するクライアントだけが送信します。セッションを持たない
`POST /auth/root/login` と、Cookie を使わない MCP の OAuth トークンエンドポイントは、
このポリシーの対象外です。

## 稼働状態

| エンドポイント | 意味 |
| --- | --- |
| `GET /api/v1/health` | プロセスの生存確認。OIDC 障害時でも root 緊急ログインを提供するため、常に `200` を返します。 |
| `GET /api/v1/readiness` | 一般利用者が OIDC ログインを開始できるか。可能なら `200 {"status":"ready","oidc":"available"}`、root 限定の縮退起動中なら `503 {"status":"degraded","oidc":"unavailable"}` を返します。 |

監視やリバースプロキシで一般公開の可否を判断する場合は `readiness` を使ってください。

## OpenAPI 仕様と互換性

`GET /api/v1/openapi.json` は、バイナリに埋め込まれた OpenAPI 3.1 ドキュメントを認証なしで
返します。REST クライアントはこのドキュメントを正式な仕様として扱ってください。Cookie・
CSRF・`Origin`・Fetch Metadata は HTTP 固有のセキュリティ機構であり、MCP のツール仕様には
含まれません。

この OpenAPI ドキュメントは `v0.1.0` から互換性を保証しています。フィールド追加などの
後方互換な変更は `/api/v1` 内で行います。破壊的変更は新しいバージョンパスで行い、既存
バージョンには少なくとも 1 リリース周期の非推奨告知と移行手順を用意します。

## ノート API

ノートの正本は UTF-8 の AsciiDoc ファイルです。SQLite は検索・ACL・参照のための投影であり、
HTTP ハンドラーが SQLite へ直接本文を書き込むことはありません。

| 操作 | エンドポイント | 成功時 | 必要な権限 |
| --- | --- | --- | --- |
| 一覧 | `GET /api/v1/notes?limit=50&cursor=...` | `200`（可視ノートの ID・タイトルと次カーソル） | 認証済み利用者 |
| 作成 | `POST /api/v1/notes` | `201` と `Location` | 認証済み利用者 |
| メタデータ取得 | `GET /api/v1/notes/{note_id}` | `200`（ID・タイトル・リビジョン） | Read 以上 |
| 正本取得 | `GET /api/v1/notes/{note_id}/source` | `200`（AsciiDoc 本文と `ETag`） | Read 以上 |
| 正本更新 | `PUT /api/v1/notes/{note_id}/source` | `204` | Write 以上。`If-Match` 必須 |
| 削除準備 | `POST /api/v1/notes/{note_id}/delete-preparations` | `200`（確認トークンと被参照数） | Admin または root。`If-Match` 必須 |
| 削除確定 | `POST /api/v1/notes/delete-confirmations` | `204` | 削除準備を行った本人 |
| ACL 更新 | `PUT /api/v1/notes/{note_id}/acl/{user_id}` | `204` | Admin または root |

対象が存在しない場合、UUIDv7 として不正な場合、必要な権限がない場合は、ノートの存在を
推測させないため、いずれも同じ `404 not-found` を返します。

### 検索

`GET /api/v1/search?q=<query>&limit=50&cursor=...` は、閲覧できるノートだけを SQLite FTS5 で
検索します。

- 結果の各要素は `note_id` と `title` です。本文や一致箇所の抜粋は返しません。
- 検索語を FTS 構文として解釈することはありません。空白で区切った各語を AND 条件として
  扱います。
- `tags` は繰り返し指定でき、指定したすべてのタグを持つノートに絞り込みます。
- `created_after`・`created_before`・`updated_after`・`updated_before` には RFC 3339 の日時を
  指定できます。いずれも境界値を含みます。
- 作成者や参照方向によるフィルターは公開していません。

一覧・検索の応答は `notes` と `next_cursor` を持つオブジェクトです。カーソルには前の応答の
`next_cursor` だけを指定してください。`limit` の既定値は 50、上限は 100 です。現在のカーソルは
オフセットを不透明化した値のため、取得中に一覧の並びが変わると結果が重複・欠落することが
あります。

### 作成と更新

作成・更新のリクエストボディは AsciiDoc の本文そのものです。保存前に `marginalis-asciidoc` が
必須メタデータ、危険な構文、`xref:note:` 参照、アンカー、ソースコードブロックの言語を検証し、
検索・参照投影を作ります。

クライアントは文書ヘッダーを含む全文を送ります。`note-id`・`creator-id`・`created-at`・
`updated-at` の各属性は 1 個ずつ存在する必要がありますが、その値をサーバーは信頼しません。

- 作成時: サーバーが新しい UUIDv7、リクエストした利用者、現在時刻へ置き換えます。
- 更新時: `note-id`・`creator-id`・`created-at` が現在の正本と一致する場合だけ受理し、
  `updated-at` は常にサーバー時刻へ置き換えます。それ以外の属性・文書構造・本文は
  そのまま保持します。

したがって次は作成リクエストの構造例であり、ID や日時を事前に取得する必要はありません。

```adoc
= 研究メモ
:note-id: 01800000-0000-7000-8000-000000000001
:creator-id: 01800000-0000-7000-8000-000000000002
:created-at: 2026-07-23T00:00:00.000Z
:updated-at: 2026-07-23T00:00:00.000Z
:tags: research, idea

xref:note:01800000-0000-7000-8000-000000000003[関連ノート]
```

`POST /api/v1/notes` には、この UTF-8 本文と `X-CSRF-Token` を送ります。成功時の `Location`
は作成された正本の `/api/v1/notes/{note_id}/source` です。

更新は楽観的ロックで競合を防ぎます。正本取得時の `ETag` を、そのまま `PUT` または削除準備の
`If-Match` に指定してください。`If-Match` の欠落や現在のリビジョンとの不一致は
`409 conflict`、形式の誤りは `422 validation-failed` になります。URL の `note_id` と文書内の
`:note-id:` の不一致、既存ノートの `:creator-id:`・`:created-at:` の変更も
`422 validation-failed` です。

### 削除

削除は準備と確定の二段階です。

1. 削除準備は、タイトル、リビジョン、5 分間だけ有効な一回限りの `confirmation_token`、および
   リクエストした利用者から見える範囲の被参照数 `incoming_reference_count` を返します。
2. 削除確定のボディは `{"confirmation_token":"..."}` です。トークンは実行者・対象ノート・
   リビジョン・**全**被参照集合に結び付いており、準備後に本文や参照が変化していた場合は
   `409 conflict` になります。

確定に成功すると、正本ファイルと SQLite 上の投影・ACL・参照・確認情報を物理削除します。
復元 API や保持期間は現在のリリースでは提供しません。

### ACL 更新

ボディは `{"permission":"read"}`・`"write"`・`"admin"`、または直接付与の解除を表す
`{"permission":null}` です。最後の直接管理者を解除・降格する更新は拒否します。

## root 管理

`POST /auth/root/login` は `{"password":"..."}` を受け取り、パスワードが正しければ `204` と
root のセッション Cookie・CSRF Cookie を返します。失敗時は理由を区別せず
`401 authentication-required` を返します。

- root セッションは無操作 30 分、または発行から 8 時間で失効します。
- ログイン失敗は TCP 接続元ごとに 15 分間で 5 回までに制限し、超過時は
  `429 too-many-requests` を返します。`X-Forwarded-For` は使わないため、リバースプロキシ
  経由の構成ではプロキシが接続元として扱われます。プロキシ側の対策は
  [NixOS での運用](nixos.md)を参照してください。

| 操作 | エンドポイント | 成功時 | 認可 |
| --- | --- | --- | --- |
| 保留 OIDC ユーザー一覧 | `GET /api/v1/admin/users/pending` | `200`（ユーザー配列） | root |
| 保留 OIDC ユーザー有効化 | `PUT /api/v1/admin/users/{user_id}/activate` | `204` | root、CSRF |
| 有効 OIDC ユーザー無効化 | `PUT /api/v1/admin/users/{user_id}/disable` | `204` | root、CSRF |
| 登録ポリシー取得・更新 | `GET` / `PUT /api/v1/admin/registration-policy` | `200` / `204` | root。更新は CSRF |
| MCP クライアント事前登録 | `POST /api/v1/admin/mcp-clients` | `204` | root、CSRF |
| 他ユーザーの MCP 認可取消 | `DELETE /api/v1/admin/mcp-authorizations?user_id=...&client_id=...` | `204` | root、CSRF |

- 有効化は `pending` 状態の OIDC ユーザーにだけ作用します。有効化後、そのユーザーは次回の
  OIDC ログインで通常のセッションを取得できます。
- 無効化は `active` 状態の OIDC ユーザーにだけ作用し、同一トランザクションでそのユーザーの
  Web セッション、MCP アクセストークン、リフレッシュトークンをすべて失効させます。
  無効化されたユーザーは OIDC ログイン、REST API、MCP のいずれも利用できません。
- 登録ポリシーは SQLite に永続化され、`{"policy":"open"}` または `{"policy":"approval"}` を
  指定できます。`invite-only` は招待機能が導入されるまで選択できません。
- MCP クライアント事前登録のボディは `client_id`・`display_name`・`redirect_uris` を持つ
  JSON オブジェクトです。リダイレクト URI は HTTPS、またはループバック
  （`127.0.0.1`・`localhost`・`::1`）への HTTP だけを許可し、クエリ・フラグメント・
  ユーザー情報を含んではいけません。

API クライアントは root のパスワードをコマンド引数、履歴、ログへ記録しないでください。
初回起動用シークレットの管理は [NixOS での運用](nixos.md#シークレットの扱い)に従います。
root ログインと `/api/v1/admin/*` は、通常のルーターとは独立した管理ルーターに収容されて
います。現在は同一リスナーで提供していますが、将来の専用管理オリジンや mTLS 化では、
この管理ルーターだけを別リスナーへ移します。

### 監査ログ

root のログイン成功・失敗、ログアウト、OIDC ユーザーの有効化・無効化、登録ポリシー変更、
root による MCP 管理操作は、SQLite の `root_audit_log` に記録されます。

- 閲覧用の HTTP API はありません。運用者がサーバー上で読み取り専用に参照します。
- パスワード、Cookie、セッション ID、OIDC コード、トークンおよびそのハッシュは記録しません。
- 日次の `marginalis-prune-audit.timer` が 365 日より古い行を削除します。同じ保守処理が、
  期限切れ・消費済みの OIDC ログイン試行、セッション、削除確認、MCP トークンも削除します。
  ノートの正本と投影は対象外です。

## MCP 認可の管理（一般利用者）

- `GET /api/v1/mcp-authorizations` は、ログイン中の利用者が持つ有効な MCP クライアント認可を
  返します。各要素はクライアント ID、表示名、スコープ、最初の認可時刻（`authorized_at`）、
  最後の利用時刻（`last_used_at`。未使用なら `null`）で、いずれも RFC 3339 形式です。
  トークンやそのハッシュは含まれません。
- `DELETE /api/v1/mcp-authorizations?client_id=...` で自分のクライアント認可を取り消せます。
  root は `DELETE /api/v1/admin/mcp-authorizations?user_id=...&client_id=...` で任意ユーザーの
  認可を取り消せます。いずれも CSRF トークンが必要で、対象のアクセストークンと
  リフレッシュトークンを直ちにすべて失効させます。

## ログアウト

`POST /auth/logout` は現在のセッションをサーバー側で失効させ、`marginalis_session` と
`marginalis_csrf` Cookie を削除して `204` を返します。Cookie を伴う変更操作なので CSRF、
`Origin` と、送信された場合の `Sec-Fetch-Site` の検証対象です。
